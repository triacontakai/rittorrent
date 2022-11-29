use std::os::unix::io::{OwnedFd, RawFd};
use std::os::unix::prelude::*;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::helpers::strerror;

pub type Token = usize;

pub use event::{Event, Events};
pub use interest::Interest;

mod interest {
    use std::ops::{BitOr, BitOrAssign};

    /// Thin wrapper around epoll bitflags
    ///
    /// Currently only supports EPOLLIN and EPOLLOUT (with [Interest::READABLE] and [Interest::WRITABLE] respectively)
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Interest(u32);

    impl Interest {
        pub const READABLE: Interest = Interest(libc::EPOLLIN as u32);
        pub const WRITABLE: Interest = Interest(libc::EPOLLOUT as u32);

        /// Returns epoll event flags
        pub fn flags(&self) -> u32 {
            self.0
        }
    }

    impl BitOr for Interest {
        type Output = Interest;

        fn bitor(self, rhs: Self) -> Self {
            Interest(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Interest {
        fn bitor_assign(&mut self, rhs: Self) {
            self.0 |= rhs.0;
        }
    }
}

mod event {
    use super::Token;

    /// Friendlier version of [libc::epoll_event]
    #[derive(Copy, Clone)]
    pub struct Event {
        token: Token,
        flags: u32,
    }

    impl Event {
        pub fn token(&self) -> Token {
            self.token
        }

        pub fn is_readable(&self) -> bool {
            (self.flags & libc::EPOLLIN as u32) != 0
        }

        pub fn is_writable(&self) -> bool {
            (self.flags & libc::EPOLLOUT as u32) != 0
        }

        pub fn is_error(&self) -> bool {
            (self.flags & libc::EPOLLERR as u32) != 0
        }
    }

    /// Wrapper around return value of epoll
    ///
    /// Transparently turns [libc::epoll_event] instances into [Event]
    pub struct Events {
        pub(super) vec: Vec<libc::epoll_event>,
        pub(super) num_events: usize,
    }

    impl Events {
        pub fn with_capacity(capacity: usize) -> Self {
            Events {
                vec: vec![libc::epoll_event { events: 0, u64: 0 }; capacity],
                num_events: 0,
            }
        }

        pub fn capacity(&self) -> usize {
            self.vec.len()
        }

        pub fn clear(&mut self) {
            self.num_events = 0;
        }

        pub fn iter(&self) -> Iter<'_> {
            Iter {
                events_iter: self.vec.iter().take(self.num_events),
            }
        }
    }

    pub struct Iter<'a> {
        events_iter: std::iter::Take<std::slice::Iter<'a, libc::epoll_event>>,
    }

    impl Iterator for Iter<'_> {
        type Item = Event;

        fn next(&mut self) -> Option<Self::Item> {
            self.events_iter.next().map(|e| Event {
                token: e.u64 as usize,
                flags: e.events,
            })
        }
    }
}

pub struct Poll {
    epollfd: OwnedFd,
}

impl Poll {
    /// Returns a new instance of Poll
    ///
    /// This uses epoll internally
    pub fn new() -> Result<Self> {
        // Safety: this just creates an fd (or fails), so there is nothing unsafe here
        let raw_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };

        if raw_fd == -1 {
            return Err(anyhow!("Poll::new: {}", strerror()));
        }

        // Safety: this is a valid fd because we just checked for error condition
        let epollfd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

        Ok(Poll { epollfd })
    }

    /// Registers a source (something with an fd) to be polled
    ///
    /// Pretty much the same interface as the `mio` crate except not cross platform
    pub fn register<T: AsRawFd>(&self, source: &T, token: Token, interest: Interest) -> Result<()> {
        let raw_fd = source.as_raw_fd();

        // this needs to be mut because epoll_ctl event parameter is not const
        let mut event = libc::epoll_event {
            events: interest.flags(),
            u64: token as u64,
        };

        // Safety: epoll_ctl is atomic and we have an exclusive reference to the source
        let ret = unsafe {
            libc::epoll_ctl(
                self.epollfd.as_raw_fd(),
                libc::EPOLL_CTL_ADD,
                raw_fd,
                &mut event,
            )
        };

        if ret == -1 {
            return Err(anyhow!("Poll::register: {}", strerror()));
        }

        Ok(())
    }

    pub fn poll(&mut self, events: &mut Events, timeout: Option<Duration>) -> Result<()> {
        let timeout = timeout.map(|t| t.as_secs() as i32).unwrap_or(-1);

        // Safety: events lives past this call, and events.capacity() ensures no OOB
        let num_events = unsafe {
            libc::epoll_wait(
                self.epollfd.as_raw_fd(),
                events.vec.as_mut_ptr(),
                events.capacity() as i32,
                timeout,
            )
        };

        if num_events == -1 {
            return Err(anyhow!("Poll::poll: {}", strerror()));
        }

        events.num_events = num_events as usize;

        Ok(())
    }
}
