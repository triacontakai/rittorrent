use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::{collections::HashMap, net::TcpStream};

use anyhow::{anyhow, Result};
use format_bytes::format_bytes;
use regex::Regex;
use url::Url;
use urlencoding::{encode, encode_binary};

const CRLF: &[u8] = b"\r\n";

#[derive(Debug)]
pub struct Response {
    pub status: u32,
    pub content: Vec<u8>,
    pub headers: HashMap<String, String>,
}

fn strip_leading_whitespace(s: &mut String) {
    // https://stackoverflow.com/a/57063944
    s.retain(|c| !c.is_whitespace());
}

pub fn http_get(url: &str, parameters: &[(&str, &[u8])]) -> Result<Response> {
    // First, let's try to parse the provided URL
    let parsed_url = Url::parse(url)?;
    // Is this an http url?
    if parsed_url.scheme() != "http" {
        return Err(anyhow!(
            "http_get: scheme {} is not valid",
            parsed_url.scheme()
        ));
    }

    // Next, let's try to connect to the remote
    let addrs = parsed_url.socket_addrs(|| None)?;
    let stream = TcpStream::connect(&*addrs)?;

    // Create a BufWriter and BufReader
    let mut writer = BufWriter::new(stream.try_clone()?);
    let mut reader = BufReader::new(stream.try_clone()?);

    // Send the HTTP request itself
    let path = parsed_url.path().as_bytes();
    let mut request = format_bytes!(b"GET {}", path);
    // Add the query parameters
    let mut is_first = true;
    for (query, value) in parameters {
        let query = encode(query).into_owned();
        let value = encode_binary(value).into_owned();
        let formatted = format!("{}{}={}", if is_first { "?" } else { "&" }, query, value);
        request.extend(formatted.as_bytes());

        is_first = false;
    }
    request.extend(format_bytes!(b" HTTP/1.1{}", CRLF));
    writer.write_all(&request)?;

    // Send the HTTP request headers
    let mut request_headers = HashMap::new();
    if let Some(host) = parsed_url.host() {
        request_headers.insert(String::from("Host"), host.to_string());
    } else {
        return Err(anyhow!("http_get: url has no host!"));
    }
    for (name, value) in request_headers {
        writer.write_all(&format_bytes!(b"{}: {}", name.as_bytes(), value.as_bytes()))?;
        writer.write_all(CRLF)?;
    }
    writer.write_all(CRLF)?;

    writer.flush()?;

    // Receive the HTTP response headers
    let mut response_headers = HashMap::new();
    let mut status_code: Option<u32> = None;
    let mut response_length: Option<usize> = None;

    let re_1_1: Regex = Regex::new(r"^HTTP/1.1 (\d{3})")?;
    let re_1_0: Regex = Regex::new(r"^HTTP/1.0 (\d{3})")?;
    for line in reader.by_ref().lines() {
        let line = line?;

        // Look for line with status code (HTTP 1.1)
        if let Some(captures) = re_1_1.captures(&line) {
            if let Some(status) = captures.get(1) {
                status_code = Some(status.as_str().parse()?);
            }
        }

        // Look for line with status code (HTTP 1.0)
        if let Some(captures) = re_1_0.captures(&line) {
            if let Some(status) = captures.get(1) {
                status_code = Some(status.as_str().parse()?);
            }
        }

        // If empty line, we're done with headers
        if line == "" {
            break;
        }

        if let Some((name, value)) = line.split_once(":") {
            let name = String::from(name);
            let mut value = String::from(value);

            // strip leading whitespace
            strip_leading_whitespace(&mut value);

            // actually add the header into the map
            response_headers.insert(name, value);
        }
    }

    if let Some(len) = response_headers.get("Content-Length") {
        response_length = Some(len.parse()?);
    }

    // Receive the rest of the response and return
    if let Some(status) = status_code {
        if let Some(len) = response_length {
            let mut buf = vec![0u8; len];

            reader.read_exact(&mut buf)?;

            Ok(Response {
                status: status,
                content: buf,
                headers: response_headers,
            })
        } else {
            let mut buf = Vec::new();

            reader.read_to_end(&mut buf)?;

            Ok(Response {
                status,
                content: buf,
                headers: response_headers,
            })
        }
    } else if !response_headers.contains_key("Content-Length") {
        Err(anyhow!(
            "http_get: Did not receive Content-Length in HTTP response!"
        ))
    } else if status_code.is_none() {
        Err(anyhow!(
            "http_get: Did not receive status code in HTTP response!"
        ))
    } else {
        Err(anyhow!("http_get: Unknown error"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[test]
    fn http_get_1() {
        let mut query = HashMap::new();
        query.insert("query1".to_owned(), "value1".to_owned());
        let resp = super::http_get(
            "http://128.8.126.63:21212/announce",
            &[("query1", "value1".as_bytes())],
        )
        .unwrap();
        println!("Response: {}", String::from_utf8(resp.content).unwrap());
    }
}
