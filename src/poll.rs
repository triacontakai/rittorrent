use std::os::unix::io::{OwnedFd, RawFd};
use std::os::unix::prelude::*;
use std::ptr::null;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::helpers::strerror;

mod event;
mod interest;

pub type Token = usize;

pub use event::{Event, Events};
pub use interest::Interest;

/// Struct that offers pretty much the same interface as the `mio` crate except not cross platform
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

    /// Reregisters a source, modifying the what we are monitoring
    pub fn reregister<T: AsRawFd>(&self, source: &T, token: Token, interest: Interest) -> Result<()> {
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
                libc::EPOLL_CTL_MOD,
                raw_fd,
                &mut event,
            )
        };

        if ret == -1 {
            return Err(anyhow!("Poll::reregister: {}", strerror()));
        }

        Ok(())
    }

    /// Deregisters a source, removing it from the [Poll] instance.
    pub fn deregister<T: AsRawFd>(&self, source: &T) -> Result<()> {
        let raw_fd = source.as_raw_fd();

        // Safety: epoll_ctl is atomic and we have an exclusive reference to the source
        let ret = unsafe {
            libc::epoll_ctl(
                self.epollfd.as_raw_fd(),
                libc::EPOLL_CTL_DEL,
                raw_fd,
                std::ptr::null_mut(),
            )
        };

        if ret == -1 {
            return Err(anyhow!("Poll::deregister: {}", strerror()));
        }

        Ok(())
    }

    pub fn poll(&mut self, events: &mut Events, timeout: Option<Duration>) -> Result<()> {
        let timeout = timeout.map(|t| t.as_millis() as i32).unwrap_or(-1);

        // Safety: events lives past this call, and events.capacity() ensures no OOB
        let num_events = unsafe {
            libc::epoll_wait(
                self.epollfd.as_raw_fd(),
                events.vec.as_mut_ptr() as *mut libc::epoll_event,
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
