mod helpers;
mod poll;

use std::{io, net::TcpListener};

use anyhow::Result;
use poll::{Events, Interest, Poll, Token};

const INPUT: Token = 0;
const SERVER: Token = 1;

fn main() -> Result<()> {
    let mut poll = Poll::new()?;

    let stdin = io::stdin();
    let server = TcpListener::bind("127.0.0.1:5000")?;

    poll.register(&stdin, INPUT, Interest::READABLE)?;
    poll.register(&server, SERVER, Interest::READABLE)?;

    let mut events = Events::with_capacity(20);

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                INPUT => {
                    if event.is_readable() {
                        let mut input = String::new();
                        stdin.read_line(&mut input)?;
                        println!("read line: {}", input);
                    }
                }
                SERVER => {
                    if event.is_readable() {
                        let connection = server.accept()?;
                        println!("accepted connection: {:?}", connection)
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}
