# rust-tracer-example

["Ray tracing in one weekend"](https://raytracing.github.io/) implementation in Rust that runs distributed on the cloud with [Hadean](platform.hadean.com). Forked from [this implementation](https://github.com/mdesmedt/rust_one_weekend).

Makes use of:

 - Hadean for distributing on the cloud
 - `futures::executor` and `crossbeam` for multicore
 - `bvh` for acceleration
 - `glam` for math
 - `minifb` for windowed output.

![Final render](https://user-images.githubusercontent.com/73319561/116177108-1b805f80-a6c8-11eb-932d-7a0b28d582c4.png)

# Usage

Release builds recommended for speed!

`cargo run --release -- serve` to run the web server on a single machine.

`cargo run --release --features gui -- window` to run the windowed GUI on a single machine.
