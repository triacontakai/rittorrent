use std::borrow::Borrow;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::{collections::HashMap, net::TcpStream};

use anyhow::{anyhow, Result};
use format_bytes::format_bytes;
use url::{Position, Url};

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

pub fn http_get<I, K, V>(url: &str, parameters: I) -> Result<Response>
where
    I: IntoIterator,
    I::Item: Borrow<(K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    // First, let's try to parse the provided URL
    let parsed_url = Url::parse_with_params(url, parameters)?;
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
    let path = &parsed_url[Position::BeforePath..Position::AfterQuery].as_bytes();
    let request = format_bytes!(b"GET {} HTTP/1.1{}", path, CRLF);
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

    for line in reader.by_ref().lines() {
        let line = line?;

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

    // Receive the rest of the response and return
    if let Some(len) = response_headers.get("Content-Length") {
        let len: usize = len.parse()?;
        let mut buf = vec![0u8; len];

        reader.read_exact(&mut buf)?;

        Ok(Response {
            status: 10,
            content: buf,
            headers: response_headers,
        })
    } else {
        Err(anyhow!(
            "http_get: Did not receive Content-Length in HTTP response!"
        ))
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
            &[("query1", "value1")],
        )
        .unwrap();
        println!("Response: {}", String::from_utf8(resp.content).unwrap());
    }
}
