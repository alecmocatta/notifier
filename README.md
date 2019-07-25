# notifier

[![Crates.io](https://img.shields.io/crates/v/notifier.svg?maxAge=86400)](https://crates.io/crates/notifier)
[![MIT / Apache 2.0 licensed](https://img.shields.io/crates/l/notifier.svg?maxAge=2592000)](#License)
[![Build Status](https://dev.azure.com/alecmocatta/notifier/_apis/build/status/tests?branchName=master)](https://dev.azure.com/alecmocatta/notifier/_build/latest?branchName=master)

[Docs](https://docs.rs/notifier/0.1.1)

A wrapper around platform event notification APIs (currently via [mio](https://github.com/carllerche/mio)) that can also handle high-resolution timer events, including those set (on another thread) *during* a `notifier.wait()` call.

Delivers **edge-triggered** notifications for file descriptor state changes (corresponding to `mio::Ready::readable() | mio::Ready::writable() | mio::unix::UnixReady::hup() | mio::unix::UnixReady::error()`) as well as elapsing of instants.

It's designed to be used in conjunction with a library that exhaustively collects events (e.g. connected, data in, data available to be written, remote closed, bytes acked, connection errors) upon each edge-triggered notification – for example [`tcp_typed`](https://github.com/alecmocatta/tcp_typed).

## Note

Currently doesn't support Windows.

## License
Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE.txt](LICENSE-APACHE.txt) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT.txt](LICENSE-MIT.txt) or http://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
