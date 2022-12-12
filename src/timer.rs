use std::{
    collections::BinaryHeap,
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};

use crate::threads::{self, Response};

pub type Token = u64;

pub struct TimerResponse {
    id: u64,
}

pub struct TimerRequest {
    timer_len: Duration,
    id: Token,
    repeat: bool,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Timer {
    expiration: Instant,
    timer_len: Duration,
    id: Token,
    repeat: bool,
}

pub fn spawn_timer_thread(sender: Sender<threads::Response>) -> Sender<TimerRequest> {
    let (tx, rx) = mpsc::channel::<TimerRequest>();

    thread::spawn(move || {
        let mut timers = BinaryHeap::new();

        loop {
            let timeout = timers
                .peek()
                .map(|x: &Timer| x.expiration.duration_since(Instant::now()))
                .unwrap_or(Duration::MAX);

            // see if we have a new timer to process
            if let Ok(req) = rx.recv_timeout(timeout) {
                let expiration = Instant::now()
                    .checked_add(req.timer_len)
                    .expect("Invalid timer!");
                let timer = Timer {
                    expiration,
                    timer_len: req.timer_len,
                    id: req.id,
                    repeat: req.repeat,
                };

                timers.push(timer);
            }

            // check for timer expirations
            while !timers.is_empty() {
                let next_timer = timers.peek().unwrap();

                // timer has expired if its expiration is before or the same as the current time
                if next_timer.expiration <= Instant::now() {
                    let timer = timers.pop().unwrap();

                    sender
                        .send(Response::Timer(TimerResponse { id: timer.id }))
                        .unwrap();

                    // place timer back on if it is a repeating timer
                    if timer.repeat {
                        let expiration = Instant::now()
                            .checked_add(timer.timer_len)
                            .expect("Invalid timer!");
                        timers.push(Timer {
                            expiration,
                            timer_len: timer.timer_len,
                            id: timer.id,
                            repeat: timer.repeat,
                        });
                    }
                } else {
                    // break if we have reached a timer that has not expired
                    break;
                }
            }
        }
    });

    tx
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    use crate::threads;

    use super::{spawn_timer_thread, TimerRequest};

    #[test]
    fn timer_thread_basic() {
        let (sender, receiver) = mpsc::channel();

        let timer_sender = spawn_timer_thread(sender);

        // this is terrible for testing but oh well it probably works fine
        let duration = Duration::from_millis(100);

        let new_timer = TimerRequest {
            timer_len: duration,
            id: 727,
            repeat: false,
        };

        let before = Instant::now();

        timer_sender.send(new_timer).unwrap();

        // i think this could result in this test hanging forever
        // but uh oh well
        let threads::Response::Timer(resp) = receiver.recv().unwrap() else {
            panic!("Timer did not return correct response enum variant");
        };

        assert_eq!(resp.id, 727);
        assert!(before.elapsed() >= duration);
    }
}
