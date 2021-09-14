# rust-tracer-example

["Ray tracing in one weekend"](https://raytracing.github.io/) implementation in Rust that runs distributed on the cloud with [Hadean](platform.hadean.com). Forked from [this implementation](https://github.com/mdesmedt/rust_one_weekend).

Makes use of:

 - Hadean for distributing on the cloud
 - `futures::executor` and `crossbeam` for multicore
 - `bvh` for raytracing acceleration
 - `glam` for math
 - `minifb` for windowed output

![Final render](https://user-images.githubusercontent.com/73319561/116177108-1b805f80-a6c8-11eb-932d-7a0b28d582c4.png)

# Build

`ln -s ~/.hadean/sdk/crates/hadean hadean-sdk-symlink` to set up a symlink to the Hadean SDK crate which Cargo.toml is configured to use. If you don't have the Hadean SDK you can need to remove the line from your `Cargo.toml` - unfortunately Cargo doesn't support optional dependencies without checking they exist!

`cargo build --release` to build the application. Release builds recommended for speed!

# Local Usage

`cargo run --release -- serve` to run the web server on a single machine.

`cargo run --release --features gui -- window` to run the windowed GUI on a single machine.

# Running with Hadean

Hadean allows you to run your application locally or distributed on the cloud, with no recompilation. First you need the [Hadean SDK](https://docs.hadean.com/platform/). Once you've got it:

```
$ cargo build --features 'distributed'
[...]
$ hadean run --config hadean-config.toml target/release/hadean-raytracer serve
[2021-09-14T17:17:42Z INFO ] Starting application...
[2021-09-14T17:17:42Z INFO ] 127.0.0.1.18004.0: Server starting on 0.0.0.0:28888
[...]
```

You can now visit the rendering interface webpage at http://localhost:28888!

## Running on a Hadean Cluster

For this you'll need a remote cluster configured for use with the Hadean Platform SDK which is explained [here](https://docs.hadean.com/platform/getting-started/distributing-an-app-in-the-cloud).

For this specific repository the deploy and run commands are structured like this:

`hadean cluster -n <cluster name> deploy ./target/release/hadean-raytracer` to deploy the application to the remote cluster.

`hadean cluster -n <cluster name> run ./hadean-config.toml` to run the application on the remote cluster. This is configured to run as a web server.

# Viewing the remote web server's output

To see the output of a remote raytracer run you'll need to grab the IP address of the machine that is running the web server. This can be obtained from the mesasges printed out by the remote run. As an example the final message of a run looks like this:

`[2021-09-14T15:13:17Z INFO ] 51.132.180.74.20004.0: finished receiving frames`

In this line of output the IP address is represented by the numbers `51.132.180.74`. You'll need to change this to match your remote machine's IP address.

To the IP address you need to add `:28888` which is the port of the web server: `<IP of your remote machine>:28888` eg. `51.132.180.74:28888`. Paste this into a browser and you will see the rendered output.
