use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use tokio::io;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::*;
use url::Url;

use crate::query::Query;

const FILTER_CRITERION_TIMEOUT_MS: u64 = 100;
const HEADER_TIMEOUT_MS: u64 = 500;
const BODY_TIMEOUT_MS: u64 = 1000;

pub struct Admit {
	pub incoming_stream: TcpStream,
	pub pending_header: Vec<u8>,
	pub pending_body: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum RejectReason {
	#[error("Timeout while receiving data from client")]
	Timeout(#[from] tokio::time::error::Elapsed),
	#[error(transparent)]
	IO(#[from] io::Error),
	#[error(transparent)]
	Query(#[from] crate::query::QueryError),
	#[error("Connection terminated early")]
	ConnectionTerminated,
	#[error("Malformed HTTP header: {0}")]
	MalformedHeader(&'static str),
	#[error("Bad request: {0}")]
	BadRequest(&'static str),
	#[error("Invalid ActivityStream ({0}):\n{1}")]
	InvalidRequest(&'static str, String),
	#[error("Spam detected:\n{0}")]
	Spam(String),
}

pub async fn handler(incoming_stream: TcpStream, query: Query) -> Result<Admit, RejectReason> {
	trace!("New connection from: {:?}", incoming_stream.peer_addr());

	const HEADER_FILTER_LEN: usize = 17;

	let mut body = Vec::new();
	let mut header = timeout(Duration::from_millis(FILTER_CRITERION_TIMEOUT_MS), async {
		let mut buf = Vec::with_capacity(HEADER_FILTER_LEN);
		let mut err = None;
		let mut first = true;
		loop {
			if !first {
				tokio::time::sleep(Duration::from_micros(100)).await;
			}
			first = false;
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
	.await??;

	// malformed HTTP header
	if header.len() < HEADER_FILTER_LEN {
		return Err(RejectReason::ConnectionTerminated);
	}

	// currently we only care about POST /inbox
	// TODO: make this configurable
	if header[0..HEADER_FILTER_LEN] != *b"POST /inbox HTTP/" {
		return Ok(Admit { incoming_stream, pending_header: header, pending_body: body });
	}

	// we should be able to get rest of the header in 500ms
	timeout(Duration::from_millis(HEADER_TIMEOUT_MS), async {
		let mut header_done = false;
		let mut err = None;
		let mut first = true;
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
			if !first {
				tokio::time::sleep(Duration::from_micros(100)).await;
			}
			first = false;
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
			return Err(e);
		}
		Ok(())
	})
	.await??;

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
	let content_length =
		content_length.ok_or(RejectReason::MalformedHeader("content-length not found"))?;
	let content_type =
		content_type.ok_or(RejectReason::MalformedHeader("content-type not found"))?;

	if !content_type.starts_with("application/activity+json")
		&& !content_type.starts_with("application/ld+json")
	{
		return Err(RejectReason::BadRequest("content-type not application/activity+json"));
	}

	// read body
	timeout(Duration::from_millis(BODY_TIMEOUT_MS), async {
		let mut err = None;
		let mut first = true;
		while body.len() < content_length && err.is_none() {
			if !first {
				tokio::time::sleep(Duration::from_micros(100)).await;
			}
			first = false;
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
	.await??;

	if body.len() != content_length {
		return Err(RejectReason::BadRequest("content-length mismatch"));
	}

	// we assume reverse proxy always uses HTTP/1.0 or HTTP/1.1 to forward back
	// so no need to handle encoded requests

	let ap_json = serde_json::from_slice::<Value>(&body).map_err(|_| {
		RejectReason::InvalidRequest("malformed JSON", String::from_utf8_lossy(&body).to_string())
	})?;

	// spam detection part

	// spam doesn't seem to be sending out raw malformed requests
	// fingers crossed

	// check if this is a new note
	if ap_json
		.as_object()
		.and_then(|o| o.get("type"))
		.and_then(|t| t.as_str())
		.and_then(|t| if t == "Create" || t == "create" { Some(()) } else { None })
		.is_none()
	{
		return Ok(Admit { incoming_stream, pending_header: header, pending_body: body });
	}

	let actor = ap_json
		.as_object()
		.and_then(|o| o.get("actor"))
		.and_then(|a| a.as_str())
		.and_then(|a| a.parse::<Url>().ok())
		.ok_or(RejectReason::InvalidRequest(
			"invalid actor",
			String::from_utf8_lossy(&body).to_string(),
		))?;
	let host = actor.host_str().ok_or(RejectReason::InvalidRequest(
		"invalid actor (no host)",
		String::from_utf8_lossy(&body).to_string(),
	))?;
	let instance_stats = query
		.get_instance_stats(host)
		.await?
		.ok_or(RejectReason::Spam(String::from_utf8_lossy(&body).to_string()))?;
	if instance_stats.followers < 5 && instance_stats.following < 5 {
		let user_stats = query
			.get_user(actor.as_str())
			.await?
			.ok_or(RejectReason::Spam(String::from_utf8_lossy(&body).to_string()))?;
		if user_stats.followers == 0 && user_stats.following == 0 {
			return Err(RejectReason::Spam(String::from_utf8_lossy(&body).to_string()));
		}
	}

	Ok(Admit { incoming_stream, pending_header: header, pending_body: body })
}
