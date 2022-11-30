use super::Token;

/// Friendlier version of [libc::epoll_event]
#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct Event {
    inner: libc::epoll_event,
}

impl Event {
    pub fn token(&self) -> Token {
        self.inner.u64 as Token
    }

    pub fn is_readable(&self) -> bool {
        (self.inner.events & libc::EPOLLIN as u32) != 0
    }

    pub fn is_writable(&self) -> bool {
        (self.inner.events & libc::EPOLLOUT as u32) != 0
    }

    pub fn is_error(&self) -> bool {
        (self.inner.events & libc::EPOLLERR as u32) != 0
    }
}

impl Default for Event {
    fn default() -> Self {
        Event { inner: libc::epoll_event { events: 0, u64: 0} }
    }
}

/// Wrapper around return value of epoll
///
/// Transparently turns [libc::epoll_event] instances into [Event]
pub struct Events {
    pub(super) vec: Vec<Event>,
    pub(super) num_events: usize,
}

impl Events {
    pub fn with_capacity(capacity: usize) -> Self {
        Events {
            vec: vec![Default::default(); capacity],
            num_events: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.num_events
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
    events_iter: std::iter::Take<std::slice::Iter<'a, Event>>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Event;

    fn next(&mut self) -> Option<Self::Item> {
        self.events_iter.next()
    }
}