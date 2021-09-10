use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, Running, StreamHandler};
use actix_web::{App, Error, HttpResponse, HttpRequest, HttpServer};
use actix_web::dev::BodyEncoding;
use actix_web::http::header::{ContentEncoding, ContentType};
use actix_web::middleware;
use actix_web::web;
use actix_web_actors::ws;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Duration;
use std::sync::{Arc, Mutex, MutexGuard};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::parallel::{self, ParallelExecutor};
use crate::render;
use crate::shared::{ColorDisplay, Point3, color_display_from_render, index_from_xy, u8_vec_from_buffer_display};

use super::{one_weekend_cam_lookat, one_weekend_scene, write_png};

static INDEX_HTML: &[u8] = include_bytes!("../static/index.html");

const WIDTH: usize = 1280/4;
const HEIGHT: usize = 720/4;
const SAMPLES_PER_PIXEL: u32 = 128/4;

// Want to range from -5 to +5
const NUM_FRAMES: usize = 40;
const DELTA_INCREMENT: f32 = 10. / NUM_FRAMES as f32;

#[derive(Serialize, Deserialize)]
struct RenderJob {}

impl RenderJob {
    fn total_frames(&self) -> usize {
        NUM_FRAMES
    }
}

struct RenderFrame {
    pixels: Vec<u8>,
    png: Vec<u8>,
}

struct RenderStatus {
    job: RenderJob,
    frames: Vec<RenderFrame>,
    gif: Option<Vec<u8>>,
    total_frames: usize,
}

#[derive(Debug)]
enum ClientState {
    NeedsConfig,
    NeedsFrame(usize),
    NeedsGif,
    Complete,
}

struct MyServerDataInner {
    clients: HashMap<Addr<MyWs>, ClientState>,
    job_tx: crossbeam::channel::Sender<RenderJob>,
    render: RenderStatus,
}

#[derive(Clone)]
struct MyServerData {
    inner: Arc<Mutex<MyServerDataInner>>,
}
impl MyServerData {
    fn lock(&self) -> MutexGuard<'_, MyServerDataInner> {
        self.inner.lock().unwrap()
    }
    fn with<F: FnOnce(&mut MyServerDataInner) -> T, T: 'static>(&self, f: F) -> T {
        f(&mut self.inner.lock().unwrap())
    }
}
type ServerData = web::Data<MyServerData>;

struct MyWs {
    state: MyServerData,
}

enum MyMsg {
    Frame(Vec<u8>),
    Reset(usize),
}

impl Message for MyMsg {
    type Result = ();
}

impl Handler<MyMsg> for MyWs {
    type Result = ();

    fn handle(&mut self, msg: MyMsg, ctx: &mut Self::Context) {
        match msg {
            MyMsg::Frame(d) => ctx.binary(d),
            MyMsg::Reset(total_frames) =>
                ctx.text(serde_json::json!({
                    "width": WIDTH,
                    "height": HEIGHT,
                    "total_frames": total_frames,
                }).to_string()),
        }
    }
}

impl Actor for MyWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        println!("starting a websocket stream");
        let addr = ctx.address();
        // Stash away the current client in our master structure
        let prev = self.state.lock().clients.insert(addr, ClientState::NeedsConfig);
        assert!(prev.is_none())
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        println!("stopping a websocket stream");
        let addr = ctx.address();
        let prev = self.state.lock().clients.remove(&addr);
        assert!(prev.is_some());
        Running::Stop
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Ok(msg) => msg,
            Err(_) => {
                ctx.close(None);
                ctx.stop();
                return
            }
        };
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Text(msg) => {
                let job: RenderJob = match serde_json::from_str(&msg) {
                    Ok(j) => j,
                    Err(e) => {
                        println!("failed to handle text ws message {:?}: {}", msg, e);
                        return
                    },
                };
                self.state.lock().job_tx.send(job).unwrap()
            },
            ws::Message::Close(_) => {
                ctx.close(None);
                ctx.stop();
            },
            v => println!("got an unhandled message {:?}", v),
        }
    }
}

async fn ws(state: ServerData, req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let resp = ws::start(MyWs { state: (**state).clone() }, &req, stream);
    println!("{:?}", resp);
    resp
}

async fn index() -> HttpResponse {
    HttpResponse::Ok().set(ContentType::html()).encoding(ContentEncoding::Gzip).body(INDEX_HTML)
}

pub fn main(addr: String) {
    let (job_tx, job_rx) = crossbeam::channel::unbounded();
    job_tx.send(RenderJob {}).unwrap();
    let state = MyServerData {
        inner: Arc::new(Mutex::new(
            MyServerDataInner {
                clients: HashMap::new(),
                job_tx,
                render: RenderStatus {
                    job: RenderJob {},
                    frames: vec![],
                    gif: None,
                    total_frames: 0,
                },
            }
        ))
    };

    let app_state = state.clone();
    let app_factory = move || {
        let app = App::new();
        let app = app.data(app_state.clone());
        let app = app.wrap(middleware::Logger::default());
        let app = app.wrap(middleware::Compress::new(ContentEncoding::Auto));
        let app = app.route("/ws", web::get().to(ws));
        let app = app.route("/", web::get().to(index));
        app
    };

    let thread_state = state.clone();
    let ref should_stop = Arc::new(AtomicBool::new(false));
    crossbeam::scope(move |scope| {
        scope.spawn(move |_| {
            let mut pool = parallel::default_pool(num_cpus::get());

            let mut scene = one_weekend_scene();
            scene.build_bvh();
            let lookat = Point3::ZERO;

            fn reset_job(job: RenderJob, state: &mut MyServerDataInner) {
                let total_frames = job.total_frames();
                state.render = RenderStatus { job, frames: vec![], gif: None, total_frames };
                // Reset clients to receive the new job config
                for (_, cs) in state.clients.iter_mut() {
                    *cs = ClientState::NeedsConfig
                }
            }

            loop {
                if should_stop.load(Ordering::SeqCst) {
                    println!("stopping rendering");
                    return
                }

                {
                    // Drain any incoming jobs
                    loop {
                        let mut state = thread_state.lock();
                        match job_rx.try_recv() {
                            Ok(job) => reset_job(job, &mut state),
                            Err(crossbeam::channel::TryRecvError::Empty) => break,
                            Err(crossbeam::channel::TryRecvError::Disconnected) => {
                                println!("ERROR channel for receiving jobs closed");
                                return
                            },
                        }
                    }
                    // Block until we find a job requiring work
                    // Careful with locking around this one, it can block forever
                    if thread_state.with(|s| s.render.frames.len() == s.render.total_frames) {
                        match job_rx.recv_timeout(Duration::from_millis(100)) {
                            Ok(job) => reset_job(job, &mut thread_state.lock()),
                            Err(crossbeam::channel::RecvTimeoutError::Timeout) => (),
                            Err(crossbeam::channel::RecvTimeoutError::Disconnected) => {
                                println!("ERROR channel for receiving jobs closed");
                                return
                            },
                        }
                        if should_stop.load(Ordering::SeqCst) {
                            println!("stopping rendering");
                            return
                        }
                    }
                }

                let (delta_idx, total_frames, has_gif) = thread_state.with(|s| (s.render.frames.len(), s.render.total_frames, s.render.gif.is_some()));
                if delta_idx != total_frames {
                    // Process next frame of current job
                    let delta_mult = (-(total_frames as f32) * DELTA_INCREMENT / 2.) + (delta_idx as f32 * DELTA_INCREMENT);
                    let cam = one_weekend_cam_lookat(WIDTH, HEIGHT, lookat + (Point3::ONE * delta_mult));
                    let render_worker = render::Renderer::new(WIDTH as u32, HEIGHT as u32, SAMPLES_PER_PIXEL, scene.clone(), cam);
                    let buffer_display = render_and_return(&render_worker, &mut pool);
                    println!("finished rendering a frame");

                    let mut png = vec![];
                    let pixels = u8_vec_from_buffer_display(&buffer_display);
                    write_png(WIDTH, HEIGHT, &mut png, &pixels);
                    thread_state.lock().render.frames.push(RenderFrame { pixels, png });
                    println!("finished creating a png");
                } else if !has_gif {
                    // We've finished all frames, create the gif
                    thread_state.with(|s| {
                        let mut gif = vec![];
                        let mut encoder = gif::Encoder::new(&mut gif, WIDTH as u16, HEIGHT as u16, &[]).unwrap();
                        encoder.set_repeat(gif::Repeat::Infinite).unwrap();
                        for frame in s.render.frames.iter() {
                            let frame = gif::Frame::from_rgb(WIDTH as u16, HEIGHT as u16, &frame.pixels);
                            encoder.write_frame(&frame).unwrap();
                        }
                        drop(encoder);
                        s.render.gif = Some(gif);
                        println!("finished creating a gif");
                    })
                }

                // Update all connected clients
                thread_state.with(|state| {
                    for (addr, cs) in state.clients.iter_mut() {
                        loop {
                            let (msg, next_cs) = match *cs {
                                // Send the config
                                ClientState::NeedsConfig => (MyMsg::Reset(state.render.total_frames), ClientState::NeedsFrame(0)),
                                // Wants more frames, but the frames are finished - move onto the gif
                                ClientState::NeedsFrame(i) if i == state.render.total_frames => {
                                    *cs = ClientState::NeedsGif;
                                    continue
                                },
                                // Wants more frames, but nothing to send yet
                                ClientState::NeedsFrame(i) if i == state.render.frames.len() => break,
                                // Send a frame
                                ClientState::NeedsFrame(i) => (MyMsg::Frame(state.render.frames[i].png.clone()), ClientState::NeedsFrame(i+1)),
                                ClientState::NeedsGif => {
                                    match state.render.gif.as_ref() {
                                        // Send the gif
                                        Some(gif) => (MyMsg::Frame(gif.clone()), ClientState::Complete),
                                        // No gif available yet
                                        None => break,
                                    }
                                },
                                // Client is up to date
                                ClientState::Complete => break,
                            };
                            // If the send was sccessful, increment the progress for this client
                            match addr.try_send(msg) {
                                Ok(()) => *cs = next_cs,
                                Err(actix::prelude::SendError::Full(_)) => {
                                    println!("failed to send to full mailbox");
                                    break
                                },
                                Err(actix::prelude::SendError::Closed(_)) => {
                                    // TODO: unregister?
                                    println!("ERROR failed to send to closed mailbox");
                                    break
                                },
                            }
                        }
                    }
                })
            }
        });

        println!("Server starting on {}", addr);
        actix_rt::System::new("actix server").block_on(async {
            HttpServer::new(app_factory)
                .bind(addr)
                .unwrap()
                .run()
                .await
        }).unwrap();
        println!("server shut down");

        should_stop.store(true, Ordering::SeqCst);
    }).unwrap();
}

// Boring all-in-one rendering of a frame
fn render_and_return(render_worker: &render::Renderer, pool: &mut impl ParallelExecutor) -> Vec<ColorDisplay> {
    render_worker.render_frame(pool);
    let mut buffer_display: Vec<ColorDisplay> = vec![0; WIDTH * HEIGHT];
    loop {
        let render_results = render_worker.poll_results();
        match render_results {
            Some(render_results) => {
                for result in render_results {
                    let index = index_from_xy(WIDTH as u32, HEIGHT as u32, result.x, result.y);
                    buffer_display[index] = color_display_from_render(result.color);
                }
            },
            None => return buffer_display,
        }
    }
}

