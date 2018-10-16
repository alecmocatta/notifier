#![allow(clippy::module_inception, clippy::option_option, clippy::cast_sign_loss, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_lossless)]

#[cfg(any(target_os = "android", target_os = "linux"))]
mod timer {
	use mio;
	use nix::{self, errno, libc, unistd};
	use std::{fmt, io, mem, os, ptr, time};
	fn u8_8_to_u64(x: [u8; 8]) -> u64 {
		unsafe { mem::transmute(x) }
	}
	pub struct Timer {
		fd: os::unix::io::RawFd,
	}
	impl Timer {
		pub fn new() -> Self {
			let fd = unsafe { libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_NONBLOCK) };
			assert_ne!(fd, -1);
			Self { fd }
		}
		pub fn set_timeout(&self, timeout: time::Instant) {
			let err = unsafe {
				libc::timerfd_settime(
					self.fd,
					libc::TFD_TIMER_ABSTIME,
					&libc::itimerspec {
						it_interval: libc::timespec {
							tv_sec: 0,
							tv_nsec: 0,
						},
						it_value: libc::timespec {
							tv_sec: timeout.as_secs() as libc::time_t,
							tv_nsec: timeout.subsec_nanos() as libc::time_t,
						},
					},
					ptr::null_mut() as *mut libc::itimerspec,
				)
			};
			assert_eq!(err, 0);
		}
		/// If the timer has elapsed since set_timeout was last called
		pub fn elapsed(&self) -> bool {
			let mut x: [u8; 8] = [0; 8];
			match unistd::read(self.fd, &mut x) {
				Ok(8) if u8_8_to_u64(x) > 0 => true,
				Err(nix::Error::Sys(errno::Errno::EAGAIN)) => false,
				e => panic!("{:?}", e),
			}
		}
	}
	impl mio::event::Evented for Timer {
		fn register(
			&self, poll: &mio::Poll, token: mio::Token, interest: mio::Ready, opts: mio::PollOpt,
		) -> io::Result<()> {
			mio::unix::EventedFd(&self.fd).register(poll, token, interest, opts)
		}
		fn reregister(
			&self, poll: &mio::Poll, token: mio::Token, interest: mio::Ready, opts: mio::PollOpt,
		) -> io::Result<()> {
			mio::unix::EventedFd(&self.fd).reregister(poll, token, interest, opts)
		}
		fn deregister(&self, poll: &mio::Poll) -> io::Result<()> {
			mio::unix::EventedFd(&self.fd).deregister(poll)
		}
	}
	impl Drop for Timer {
		fn drop(&mut self) {
			unistd::close(self.fd).unwrap();
		}
	}

	#[derive(Copy, Clone)]
	struct Timespec {
		t: libc::timespec,
	}
	#[derive(Copy, Clone)]
	struct Instant {
		t: Timespec,
	}
	impl fmt::Debug for Instant {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			f.debug_struct("Instant")
				.field("tv_sec", &self.t.t.tv_sec)
				.field("tv_nsec", &self.t.t.tv_nsec)
				.finish()
		}
	}
	impl Publicise for time::Instant {
		type Public = Instant;

		fn publicise(&self) -> &Self::Public {
			// TODO: remove this heinous hack
			let ret = unsafe { &*(self as *const Self as *const Self::Public) };
			assert_eq!(format!("{:?}", self), format!("{:?}", ret));
			ret
		}
	}
	trait Publicise {
		type Public;
		fn publicise(&self) -> &Self::Public;
	}
	trait X {
		fn as_secs(&self) -> u64;
		fn subsec_nanos(&self) -> u32;
	}
	impl X for time::Instant {
		fn as_secs(&self) -> u64 {
			self.publicise().t.t.tv_sec as u64
		}

		fn subsec_nanos(&self) -> u32 {
			self.publicise().t.t.tv_nsec as u32
		}
	}

}
#[cfg(not(any(target_os = "android", target_os = "linux")))]
mod timer {
	// alternative approach: https://github.com/jiixyj/epoll-shim/blob/master/src/timerfd.c
	use mio;
	use palaver::spawn;
	use std::{io, sync, thread, time};
	pub struct Timer {
		inner: sync::Arc<Inner>,
		thread: Option<thread::JoinHandle<()>>,
		registration: mio::Registration,
	}
	struct Inner {
		timeout: sync::Mutex<Option<Option<time::Instant>>>,
		elapsed: sync::atomic::AtomicBool,
	}
	impl Timer {
		pub fn new() -> Self {
			let timeout = sync::Mutex::new(Some(None));
			let inner = sync::Arc::new(Inner {
				timeout,
				elapsed: sync::atomic::AtomicBool::new(false),
			});
			let inner_ = inner.clone();
			let (registration, set_readiness) = mio::Registration::new2();
			let thread = spawn(String::from("deploy-timer"), move || {
				let inner = inner_;
				loop {
					let mut timeout_lock = inner.timeout.lock().unwrap();
					if timeout_lock.is_none() {
						break;
					}
					let now = time::Instant::now();
					if timeout_lock.as_ref().unwrap().is_none() {
						drop(timeout_lock);
						thread::park();
					} else if now < *timeout_lock.as_ref().unwrap().as_ref().unwrap() {
						let sleep = *timeout_lock.as_ref().unwrap().as_ref().unwrap() - now;
						drop(timeout_lock);
						thread::park_timeout(sleep);
					} else {
						*timeout_lock = Some(None);
						inner.elapsed.store(true, sync::atomic::Ordering::Relaxed);
						set_readiness.set_readiness(mio::Ready::readable()).unwrap();
					}
				}
			});
			Self {
				inner,
				thread: Some(thread),
				registration,
			}
		}
		pub fn set_timeout(&self, timeout: time::Instant) {
			self.inner
				.elapsed
				.store(false, sync::atomic::Ordering::Relaxed);
			*self.inner.timeout.lock().unwrap() = Some(Some(timeout));
			self.thread.as_ref().unwrap().thread().unpark();
		}
		/// If the timer has elapsed since set_timeout was last called
		pub fn elapsed(&self) -> bool {
			self.inner
				.elapsed
				.swap(false, sync::atomic::Ordering::Relaxed)
		}
	}
	impl mio::event::Evented for Timer {
		fn register(
			&self, poll: &mio::Poll, token: mio::Token, interest: mio::Ready, opts: mio::PollOpt,
		) -> io::Result<()> {
			<mio::Registration as mio::event::Evented>::register(
				&self.registration,
				poll,
				token,
				interest,
				opts,
			)
		}
		fn reregister(
			&self, poll: &mio::Poll, token: mio::Token, interest: mio::Ready, opts: mio::PollOpt,
		) -> io::Result<()> {
			<mio::Registration as mio::event::Evented>::reregister(
				&self.registration,
				poll,
				token,
				interest,
				opts,
			)
		}
		fn deregister(&self, poll: &mio::Poll) -> io::Result<()> {
			<mio::Registration as mio::event::Evented>::deregister(&self.registration, poll)
		}
	}
	impl Drop for Timer {
		fn drop(&mut self) {
			*self.inner.timeout.lock().unwrap() = None;
			self.thread.as_ref().unwrap().thread().unpark();
			self.thread.take().unwrap().join().unwrap();
		}
	}
}
pub use self::timer::Timer;
