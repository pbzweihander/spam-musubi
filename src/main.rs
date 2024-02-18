#![warn(clippy::unwrap_used)]

use std::{env, net::Ipv4Addr};

use clap::Parser;
use once_cell::sync::OnceCell;
use tokio::{
	io::{self, AsyncWriteExt},
	net::{TcpListener, TcpStream},
	time::Instant,
};
use tracing::*;

mod db;
mod filter;
mod query;

use query::{Query, QueryOpMode};

use crate::filter::{Filter, RejectReason};

#[derive(Parser, Debug)]
#[command(version)]
/// Layer7 firewall for ActivityPub-compatible servers.
/// IPv4 only.
///
/// Run behind nginx reverse proxy or similar.
struct Args {
	#[arg(short, long, default_value = "127.0.0.1")]
	/// Address to bind to. Leave default if you don't know what this means.
	/// "Try 0.0.0.0 if you have docker insanity.
	/// Don't forget to firewall the port properly!
	bind_address: String,
	#[arg(short, long, default_value_t = 21200)]
	/// Port to bind to. Your reverse proxy should point to this port.
	outside_port: u16,
	#[arg(short, long, default_value = "127.0.0.1")]
	/// Address of the AP server.
	ap_server_address: String,
	#[arg(short = 'p', long, default_value_t = 3000)]
	/// Port of the AP server.
	ap_server_port: u16,
	#[arg(short = 't', long, default_value = "misskey")]
	/// What server are we using?
	server_type: QueryOpMode,
}

static AP_SERVER: OnceCell<(Ipv4Addr, u16)> = OnceCell::new();
static HOST: OnceCell<String> = OnceCell::new();

#[tokio::main]
async fn main() {
	dotenvy::dotenv().ok();
	let args = Args::parse();
	#[allow(clippy::unwrap_used)]
	let bind_address: Ipv4Addr = args.bind_address.parse().unwrap();
	#[allow(clippy::unwrap_used)]
	AP_SERVER.set((args.ap_server_address.parse().unwrap(), args.ap_server_port)).unwrap();

	match env::var("RUST_LOG") {
		Ok(_) => {}
		Err(_) => env::set_var("RUST_LOG", "info"),
	}
	tracing_subscriber::fmt::init();

	info!("Cooking");

	#[allow(clippy::unwrap_used)]
	let query = Query::init(
		&std::env::var("DB_HOST").unwrap(),
		std::env::var("DB_PORT").unwrap().parse().unwrap(),
		&std::env::var("DB_USER").unwrap(),
		&std::env::var("DB_PASSWORD").unwrap(),
		&std::env::var("DB_NAME").unwrap(),
		args.server_type,
	)
	.await
	.unwrap();

	let filter = Filter::builder().build();

	let listener = TcpListener::bind((bind_address, args.outside_port))
		.await
		.expect("Could not bind to said address & port. Is the port in use?");

	loop {
		if let Ok((stream, _)) = listener.accept().await {
			let query = query.clone();
			let filter = filter.clone();
			tokio::spawn(async move {
				let now = Instant::now();
				match filter.handler(stream, query).await {
					Ok(mut admit) => {
						debug!("Accepted (in {}us)", now.elapsed().as_micros());
						match TcpStream::connect((AP_SERVER.wait().0, AP_SERVER.wait().1)).await {
							Ok(mut server_stream) => {
								if let Err(_e) =
									server_stream.write_all(&admit.pending_header).await
								{
									warn!("Could not write header to AP server");
									return;
								}
								if !admit.pending_body.is_empty() {
									if let Err(_e) =
										server_stream.write_all(&admit.pending_body).await
									{
										warn!("Could not write body to AP server");
										return;
									}
								}
								io::copy_bidirectional(
									&mut admit.incoming_stream,
									&mut server_stream,
								)
								.await
								.ok();
							}
							_ => {
								warn!("Could not connect to AP server");
							}
						}
					}
					Err(reason) => {
						info!(
							"Rejected (in {}us): {}",
							now.elapsed().as_micros(),
							match &reason {
								RejectReason::Spam(actor, _) => format!("Spam from {}", actor),
								_ => format!("{}", &reason),
							}
						);
						debug!("{}", reason);
					}
				}
			})
		} else {
			continue;
		};
	}
}
