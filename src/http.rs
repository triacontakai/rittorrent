use std::borrow::Borrow;
use std::io::Write;
use std::time::Duration;
use std::{collections::HashMap, io, net::TcpStream};

use anyhow::{anyhow, Result};
use url::{ParseError, Position, Url};

const TIMEOUT: Duration = Duration::from_secs(5);
const CRLF: &[u8] = b"\r\n";

#[derive(Debug)]
pub struct Response {
    status: u32,
    content: Vec<u8>,
    headers: HashMap<String, String>,
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
    let mut stream = TcpStream::connect(&*addrs)?;
    stream.set_write_timeout(Some(TIMEOUT))?;

    // Send the path
    let path = &parsed_url[Position::BeforePath..Position::AfterQuery];
    stream.write(b"GET ")?;
    stream.write(path.as_bytes())?;
    stream.write(b" HTTP/1.1")?;
    stream.write(CRLF)?;

    // skip headers for now
    stream.write(CRLF)?;

    // TODO: delete this
    Ok(Response {
        status: 10,
        content: Vec::new(),
        headers: HashMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[test]
    fn http_get_1() {
        let mut query = HashMap::new();
        query.insert("query1".to_owned(), "value1".to_owned());
        super::http_get("http://128.8.126.63:21212/announce", &[("query1", "value1")]).unwrap();
    }
}
