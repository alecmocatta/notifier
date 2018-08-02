# notifier

[![Crates.io](https://img.shields.io/crates/v/notifier.svg?style=flat-square&maxAge=86400)](https://crates.io/crates/notifier)
[![Apache-2.0 licensed](https://img.shields.io/crates/l/notifier.svg?style=flat-square&maxAge=2592000)](LICENSE.txt)
[![Build Status](https://circleci.com/gh/alecmocatta/notifier/tree/master.svg?style=shield)](https://circleci.com/gh/alecmocatta/notifier)
[![Build Status](https://travis-ci.com/alecmocatta/notifier.svg?branch=master)](https://travis-ci.com/alecmocatta/notifier)

[Docs](https://docs.rs/crate/notifier/0.1.0)

A wrapper around platform event notification APIs (currently via [mio](https://github.com/carllerche/mio)) that can also handle high-resolution timer events, including those set (on another thread) *during* a `notifier.wait()` call.

Delivers **edge-triggered** notifications for file descriptor state changes (corresponding to `mio::Ready::readable() | mio::Ready::writable() | mio::unix::UnixReady::hup() | mio::unix::UnixReady::error()`) as well as elapsing of instants.

It's designed to be used in conjunction with a library that exhaustively collects events (e.g. connected, data in, data available to be written, remote closed, bytes acked, connection errors) upon each edge-triggered notification – for example [`tcp_typed`](https://github.com/alecmocatta/tcp_typed).

## Note

Currently doesn't support Windows.

## License

Licensed under Apache License, Version 2.0, ([LICENSE.txt](LICENSE.txt) or http://www.apache.org/licenses/LICENSE-2.0).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be licensed as above, without any additional terms or conditions.
