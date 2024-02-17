#![warn(clippy::unwrap_used)]

use std::{net::Ipv4Addr, time::Duration};

use clap::Parser;
use once_cell::sync::OnceCell;
use serde_json::Value;
use tokio::{
	io::{self, AsyncWriteExt},
	net::{TcpListener, TcpStream},
	time::timeout
};
use tracing::*;
use url::Url;

const FILTER_CRITERION_TIMEOUT_MS: u64 = 100;
const HEADER_TIMEOUT_MS: u64 = 500;
const BODY_TIMEOUT_MS: u64 = 1000;

mod db;
mod query;

use query::Query;

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
	ap_server_port: u16
}

static AP_SERVER: OnceCell<(Ipv4Addr, u16)> = OnceCell::new();

#[tokio::main]
async fn main() {
	dotenvy::dotenv().ok();
	let args = Args::parse();
	#[allow(clippy::unwrap_used)]
	let bind_address: Ipv4Addr = args.bind_address.parse().unwrap();
	#[allow(clippy::unwrap_used)]
	AP_SERVER.set((args.ap_server_address.parse().unwrap(), args.ap_server_port)).unwrap();

	tracing_subscriber::fmt::init();

	#[allow(clippy::unwrap_used)]
	let query = Query::init(
		&std::env::var("DB_HOST").unwrap(),
		&std::env::var("DB_USER").unwrap(),
		&std::env::var("DB_PASSWORD").unwrap(),
		&std::env::var("DB_NAME").unwrap()
	)
	.unwrap();

	let listener = TcpListener::bind((bind_address, args.outside_port))
		.await
		.expect("Could not bind to said address & port. Is the port in use?");

	loop {
		if let Ok((stream, _)) = listener.accept().await {
			let query = query.clone();
			tokio::spawn(async move {
				handler(stream, query).await;
			})
		} else {
			continue;
		};
	}
}

async fn handler(mut incoming_stream: TcpStream, query: Query) {
	info!("New connection from: {:?}", incoming_stream.peer_addr());

	const HEADER_FILTER_LEN: usize = 17;

	let mut body = Vec::new();
	// outer Ok() is timeout, inner Ok() is TCP error
	#[allow(clippy::blocks_in_conditions)]
	let mut header = match timeout(Duration::from_millis(FILTER_CRITERION_TIMEOUT_MS), async {
		let mut buf = Vec::with_capacity(HEADER_FILTER_LEN);
		let mut err = None;
		loop {
			tokio::time::sleep(Duration::from_millis(10)).await;
			match incoming_stream.try_read_buf(&mut buf) {
				Ok(0) => break,
				Ok(_) => {
					if buf.len() > HEADER_FILTER_LEN {
						break;
					}
					continue;
				}
				Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
				Err(e) => {
					err = Some(e);
					break;
				}
			}
		}
		if let Some(e) = err {
			info!("Error reading header: {:?}", e);
			return Err(e);
		}
		Ok(buf)
	})
	.await
	{
		Ok(Ok(header)) => header,
		_ => {
			info!("Timeout reading header");
			return;
		}
	};

	// malformed HTTP header
	if header.len() < HEADER_FILTER_LEN {
		info!("Malformed HTTP header");
		return;
	}

	// currently we only care about POST /inbox
	if header[0..HEADER_FILTER_LEN] == *b"POST /inbox HTTP/" {
		// we should be able to get rest of the header in 500ms
		if let Err(e) = timeout(Duration::from_millis(HEADER_TIMEOUT_MS), async {
			let mut header_done = false;
			let mut err = None;
			while !header_done && err.is_none() {
				for (i, rnrn) in header.windows(4).enumerate() {
					if rnrn == *b"\r\n\r\n" {
						header_done = true;
						body.extend_from_slice(&header[i + 4..]);
						header.truncate(i + 4);
						break;
					}
				}
				if header_done {
					break;
				}
				tokio::time::sleep(Duration::from_millis(10)).await;
				match incoming_stream.try_read_buf(&mut header) {
					Ok(0) => break,
					Ok(_) => continue,
					Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
					Err(e) => {
						err = Some(e);
						break;
					}
				}
			}
			if let Some(e) = err {
				info!("Error reading header 2: {:?}", e);
				return Err(e);
			}
			Ok(())
		})
		.await
		{
			info!("Timeout reading header 2: {:?}", e);
			return;
		}

		// get content-length & content-type
		let mut content_length = None;
		let mut content_type = None;
		for line in header.split(|&x| x == b'\n') {
			if content_length.is_some() && content_type.is_some() {
				break;
			}
			if content_length.is_none()
				&& (line.starts_with(b"Content-Length: ") || line.starts_with(b"content-length: "))
			{
				content_length = std::str::from_utf8(&line[16..line.len() - 1])
					.ok()
					.and_then(|x| x.parse::<usize>().ok());
			}
			if content_type.is_none()
				&& (line.starts_with(b"Content-Type: ") || line.starts_with(b"content-type: "))
			{
				content_type = std::str::from_utf8(&line[14..line.len() - 1]).ok();
			}
		}
		let content_length = match content_length {
			Some(content_length) => content_length,
			// i don't think requests to /inbox will actually have chunked encoding
			_ => {
				info!("content-length");
				return;
			}
		};
		let content_type = if let Some(content_type) = content_type {
			content_type
		} else {
			info!("content-type");
			return;
		};

		if !content_type.starts_with("application/activity+json")
			&& !content_type.starts_with("application/ld+json")
		{
			info!("content-type mismatch");
			return;
		}

		// read body
		if let Err(e) = timeout(Duration::from_millis(BODY_TIMEOUT_MS), async {
			let mut err = None;
			while body.len() < content_length && err.is_none() {
				tokio::time::sleep(Duration::from_millis(10)).await;
				match incoming_stream.try_read_buf(&mut body) {
					Ok(0) => break,
					Ok(_) => continue,
					Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
					Err(e) => {
						err = Some(e);
						break;
					}
				}
			}
			if let Some(e) = err {
				info!("Error reading body: {:?}", e);
				return Err(e);
			}
			Ok(())
		})
		.await
		{
			info!("Timeout reading body: {:?}", e);
			return;
		}

		if body.len() != content_length {
			info!("content-length mismatch");
			return;
		}

		// we assume reverse proxy always uses HTTP/1.0 or HTTP/1.1 to forward back
		// so no encoded requests

		let ap_json: Value = match serde_json::from_slice(&body) {
			Ok(ap_json) => ap_json,
			Err(_) => {
				info!("Invalid JSON");
				return;
			}
		};

		// spam detection part

		// spam doesn't seem to be sending out raw malformed requests
		// fingers crossed

		// check if this is a new note
		if ap_json
			.as_object()
			.and_then(|o| o.get("type"))
			.and_then(|t| t.as_str())
			.and_then(|t| if t == "Create" { Some(()) } else { None })
			.is_some()
		{
			let actor = match ap_json
				.as_object()
				.and_then(|o| o.get("actor"))
				.and_then(|a| a.as_str())
				.and_then(|a| a.parse::<Url>().ok())
			{
				Some(actor) => actor,
				_ => {
					info!("Invalid actor");
					return;
				}
			};
			let host = match actor.host_str() {
				Some(host) => host,
				_ => {
					info!("Invalid host");
					return;
				}
			};
			let instance_stats = match query.get_instance_stats(host).await {
				Ok(Some(instance_stats)) => instance_stats,
				_ => {
					info!("Instance not found: {}", host);
					return;
				}
			};
			if instance_stats.followers == 0 && instance_stats.following == 0 {
				let user_stats = match query.get_user(actor.as_str()).await {
					Ok(Some(user_stats)) => user_stats,
					_ => {
						info!("User not found: {}", actor);
						return;
					}
				};
				if user_stats.followers == 0 && user_stats.following == 0 {
					info!("Spam detected: {}", actor);
					return;
				}
			}
		}
	}

	info!("Attempting Relay");

	// passed all checks, admit
	let mut server_stream = match TcpStream::connect((AP_SERVER.wait().0, AP_SERVER.wait().1)).await
	{
		Ok(server_stream) => server_stream,
		_ => {
			info!("Could not connect to AP server");
			return;
		}
	};

	// send existing data
	if let Err(_e) = server_stream.write_all(&header).await {
		info!("Could not write header to AP server");
		return;
	}
	if !body.is_empty() {
		if let Err(_e) = server_stream.write_all(&body).await {
			info!("Could not write body to AP server");
			return;
		}
	}

	info!("Relaying");

	// this buffers, if issues arise, just spawn two threads for each direction
	// and use io::copy_buf with buffer size of 1
	io::copy_bidirectional(&mut incoming_stream, &mut server_stream).await.ok();
}
