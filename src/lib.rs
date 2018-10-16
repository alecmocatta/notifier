//! A wrapper around platform event notification APIs (currently via [mio](https://github.com/carllerche/mio)) that can also handle high-resolution timer events, including those set (on another thread) *during* a `notifier.wait()` call.
//!
//! **[Crates.io](https://crates.io/crates/notifier) │ [Repo](https://github.com/alecmocatta/notifier)**
//!
//! Delivers **edge-triggered** notifications for file descriptor state changes (corresponding to `mio::Ready::readable() | mio::Ready::writable() | mio::unix::UnixReady::hup() | mio::unix::UnixReady::error()`) as well as elapsing of instants.
//!
//! It's designed to be used in conjunction with a library that exhaustively collects events (e.g. connected, data in, data available to be written, remote closed, bytes acked, connection errors) upon each edge-triggered notification – for example [`tcp_typed`](https://github.com/alecmocatta/tcp_typed).
//!
//! # Note
//!
//! Currently doesn't support Windows.

#![doc(html_root_url = "https://docs.rs/notifier/0.1.0")]
#![warn(
	// missing_copy_implementations,
	// missing_debug_implementations,
	// missing_docs,
	trivial_numeric_casts,
	unused_extern_crates,
	unused_import_braces,
	unused_qualifications,
	unused_results,
	clippy::pedantic
)] // from https://github.com/rust-unofficial/patterns/blob/master/anti_patterns/deny-warnings.md
#![allow(
	clippy::new_without_default,
	clippy::indexing_slicing,
	clippy::needless_pass_by_value,
	clippy::inline_always
)]

extern crate either;
extern crate mio;
#[cfg(any(target_os = "android", target_os = "linux"))]
extern crate nix;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
extern crate palaver;
#[cfg(feature = "tcp_typed")]
extern crate tcp_typed;
// #[cfg(windows)]
// extern crate winapi;
#[macro_use]
extern crate log;

mod heap;
mod timer;

use either::Either;
use std::{cmp, collections::HashSet, marker, mem, sync, time};

#[cfg(unix)]
type Fd = std::os::unix::io::RawFd;
#[cfg(windows)]
type Fd = std::os::windows::io::RawHandle;

pub struct NotifierContext<'a, Key: 'a>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	executor: &'a Notifier<Key>,
	key: Key,
}
impl<'a, Key: 'a> NotifierContext<'a, Key>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	#[inline(always)]
	pub fn add_trigger(&self) -> (Triggerer, Triggeree) {
		self.executor.add_trigger(self.key.clone())
	}
	#[inline(always)]
	pub fn queue(&self) {
		let _ = self.add_instant(time::Instant::now());
	}
	#[inline(always)]
	pub fn add_fd(&self, fd: Fd) {
		self.executor.add_fd(fd, self.key.clone())
	}
	#[inline(always)]
	pub fn remove_fd(&self, fd: Fd) {
		self.executor.remove_fd(fd, self.key.clone())
	}
	#[inline(always)]
	pub fn add_instant(&self, instant: time::Instant) -> heap::Slot {
		self.executor.add_instant(instant, self.key.clone())
	}
	#[inline(always)]
	pub fn remove_instant(&self, slot: heap::Slot) {
		self.executor.remove_instant(slot)
	}
}

#[cfg(feature = "tcp_typed")]
impl<'a, Key: 'a> tcp_typed::Notifier for NotifierContext<'a, Key>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	type InstantSlot = heap::Slot;
	#[inline(always)]
	fn queue(&self) {
		self.queue()
	}
	#[inline(always)]
	fn add_fd(&self, fd: Fd) {
		self.add_fd(fd)
	}
	#[inline(always)]
	fn remove_fd(&self, fd: Fd) {
		self.remove_fd(fd)
	}
	#[inline(always)]
	fn add_instant(&self, instant: time::Instant) -> heap::Slot {
		self.add_instant(instant)
	}
	#[inline(always)]
	fn remove_instant(&self, slot: heap::Slot) {
		self.remove_instant(slot)
	}
}

struct TimeEvent<Key>(time::Instant, Key);
impl<Key> PartialEq for TimeEvent<Key> {
	#[inline(always)]
	fn eq(&self, other: &Self) -> bool {
		self.0.eq(&other.0)
	}
}
impl<Key> Eq for TimeEvent<Key> {}
impl<Key> PartialOrd for TimeEvent<Key> {
	#[inline(always)]
	fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
		Some(self.0.cmp(&other.0))
	}
}
impl<Key> Ord for TimeEvent<Key> {
	#[inline(always)]
	fn cmp(&self, other: &Self) -> cmp::Ordering {
		self.0.cmp(&other.0)
	}
}
pub struct Notifier<Key>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	notifier_timeout: NotifierTimeout<Key>,
	// queue: Vec<Key>,
	timer: sync::RwLock<heap::Heap<TimeEvent<Key>>>,
}
impl<Key> Notifier<Key>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	pub fn new() -> Self {
		Self {
			notifier_timeout: NotifierTimeout::new(),
			// queue: Vec::new(),
			timer: sync::RwLock::new(heap::Heap::new()),
		}
	}

	pub fn context(&self, key: Key) -> NotifierContext<Key> {
		NotifierContext {
			executor: self,
			key,
		}
	}

	fn add_fd(&self, fd: Fd, data: Key) {
		self.notifier_timeout.add(
			&mio::unix::EventedFd(&fd),
			mio::Ready::readable()
				| mio::Ready::writable()
				| mio::unix::UnixReady::hup()
				| mio::unix::UnixReady::error(), // EPOLLRDHUP?
			data,
		);
	}

	fn remove_fd(&self, fd: Fd, data: Key) {
		self.notifier_timeout
			.delete(&mio::unix::EventedFd(&fd), data);
	}

	fn add_instant(&self, instant: time::Instant, data: Key) -> heap::Slot {
		trace!("add_instant {:?}", instant);
		let mut timer = self.timer.write().unwrap();
		let slot = timer.push(TimeEvent(instant, data));
		self.notifier_timeout.update_timeout(instant);
		slot
	}

	fn remove_instant(&self, slot: heap::Slot) {
		let _ = self.timer.write().unwrap().remove(slot); // TODO
	}

	fn add_trigger(&self, data: Key) -> (Triggerer, Triggeree) {
		let (registration, set_readiness) = mio::Registration::new2();
		self.notifier_timeout
			.add(&registration, mio::Ready::readable(), data);
		(Triggerer(set_readiness), Triggeree(registration))
	}

	pub fn wait<F: FnMut(Either<mio::Ready, time::Instant>, Key)>(&self, mut f: F) {
		let mut done_any = false;
		let now = time::Instant::now();
		let timeout = {
			loop {
				let TimeEvent(timeout, poll_key) = {
					let timer = &mut *self.timer.write().unwrap();
					if timer.peek().is_some() && timer.peek().unwrap().0 <= now {
						trace!(
							"timeout unelapsed {:?} <= {:?}",
							timer.peek().unwrap().0,
							now
						);
					}
					if timer.peek().is_none() || timer.peek().unwrap().0 > now {
						break;
					}
					timer.pop().unwrap()
				};
				done_any = true;
				trace!("ran timeout {:?}", timeout);
				f(Either::Right(timeout), poll_key)
			}
			self.timer.read().unwrap().peek().map(|x| x.0)
		};
		trace!("\\wait {:?}", timeout);
		if let Some(timeout) = timeout {
			self.notifier_timeout.update_timeout(timeout);
		}
		self.notifier_timeout
			.wait(done_any, |flags, poll_key| f(Either::Left(flags), poll_key));
		trace!("/wait");
		let now = time::Instant::now();
		loop {
			let TimeEvent(timeout, poll_key) = {
				let timer = &mut *self.timer.write().unwrap();
				if timer.peek().is_some() && timer.peek().unwrap().0 <= now {
					trace!(
						"timeout unelapsed {:?} <= {:?}",
						timer.peek().unwrap().0,
						now
					);
				}
				if timer.peek().is_none() || timer.peek().unwrap().0 > now {
					break;
				}
				timer.pop().unwrap()
			};
			trace!("ran timeout {:?}", timeout);
			f(Either::Right(timeout), poll_key)
		}
	}
}
pub struct Triggerer(mio::SetReadiness);
impl Drop for Triggerer {
	fn drop(&mut self) {
		self.0.set_readiness(mio::Ready::readable()).unwrap();
	}
}
pub struct Triggeree(mio::Registration);

const POLL_BUF_LENGTH: usize = 100;
const POLL_TIMER: mio::Token = mio::Token(usize::max_value() - 1); // max_value() is taken by mio

struct NotifierTimeout<Key>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	poll: mio::Poll,
	timer: timer::Timer,
	timeout: sync::Mutex<Option<time::Instant>>,
	strip: sync::Mutex<Option<HashSet<usize>>>,
	marker: marker::PhantomData<fn(Key)>,
}
impl<Key> NotifierTimeout<Key>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	fn new() -> Self {
		let poll = mio::Poll::new().unwrap();
		let timer = timer::Timer::new();
		poll.register(
			&timer,
			POLL_TIMER,
			mio::Ready::readable(),
			mio::PollOpt::edge(),
		)
		.unwrap();
		Self {
			poll,
			timer,
			timeout: sync::Mutex::new(None),
			strip: sync::Mutex::new(None),
			marker: marker::PhantomData,
		}
	}

	fn add<E: mio::event::Evented + ?Sized>(&self, fd: &E, events: mio::Ready, data: Key) {
		let data: usize = data.into();
		assert_ne!(mio::Token(data), POLL_TIMER);
		if let Some(ref mut strip) = *self.strip.lock().unwrap() {
			let _ = strip.remove(&data);
		}
		self.poll
			.register(fd, mio::Token(data), events, mio::PollOpt::edge())
			.unwrap();
	}

	fn delete<E: mio::event::Evented + ?Sized>(&self, fd: &E, data: Key) {
		self.poll.deregister(fd).unwrap();
		if let Some(ref mut strip) = *self.strip.lock().unwrap() {
			let x = strip.insert(data.into());
			assert!(x);
		}
	}

	fn update_timeout(&self, timeout: time::Instant) {
		let mut current_timeout = self.timeout.lock().unwrap();
		trace!("update_timeout {:?} {:?}", current_timeout, timeout);
		if current_timeout.is_none() || timeout < current_timeout.unwrap() {
			*current_timeout = Some(timeout);
			self.timer.set_timeout(timeout);
		}
	}

	fn wait<F: FnMut(mio::Ready, Key)>(&self, mut nonblock: bool, mut f: F) {
		let mut events = mio::Events::with_capacity(POLL_BUF_LENGTH);
		loop {
			let x = mem::replace(&mut *self.strip.lock().unwrap(), Some(HashSet::new()));
			assert!(x.is_none());
			let n = loop {
				trace!("\\mio_wait {:?}", nonblock);
				let n = self
					.poll
					.poll(
						&mut events,
						if nonblock {
							Some(time::Duration::new(0, 0))
						} else {
							None
						},
					)
					.unwrap();
				trace!("/mio_wait: {:?}", n);
				if !nonblock && n == 0 {
					continue;
				}
				let mut current_timeout = self.timeout.lock().unwrap();
				if self.timer.elapsed() {
					*current_timeout = None;
				}
				break n;
			};
			assert!(n <= events.capacity());
			let strip = mem::replace(&mut *self.strip.lock().unwrap(), None).unwrap(); // TODO: currently Context needs to do its own check for strips added after this point
			for x in events
				.iter()
				.filter(|x| x.token() != POLL_TIMER && !strip.contains(&x.token().0))
			{
				f(x.readiness(), x.token().0.into())
			}
			if n < events.capacity() {
				break;
			}
			nonblock = true;
		}
	}
}
impl<Key> Drop for NotifierTimeout<Key>
where
	Key: Clone + Into<usize>,
	usize: Into<Key>,
{
	fn drop(&mut self) {
		self.poll.deregister(&self.timer).unwrap();
	}
}
