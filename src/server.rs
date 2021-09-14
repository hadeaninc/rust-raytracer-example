use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, Running, StreamHandler};
use actix_web::{App, Error, HttpResponse, HttpRequest, HttpServer};
use actix_web::dev::BodyEncoding;
use actix_web::http::header::{ContentEncoding, ContentType};
use actix_web::middleware;
use actix_web::web;
use actix_web_actors::ws;
use futures::prelude::*;
use image::GenericImage;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Duration;
use std::sync::{Arc, Mutex, MutexGuard};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::parallel::{self, ParallelExecutor};
use crate::render;
use crate::scene::Scene;
use crate::shared::Point3;
use crate::{one_weekend_cam_lookat, one_weekend_scene};

static INDEX_HTML: &[u8] = include_bytes!("../static/index.html");

const THUMB_MAX_PX: u32 = 50;

// Want to range from -5 to +5
const PAN_RANGE: f32 = 10.;

#[derive(Clone)]
#[derive(Serialize, Deserialize)]
struct RenderJob {
    total_frames: usize,
    samples_per_pixel: u32,
    width: u16,
    height: u16,
    parallel: ParallelType,
}

#[derive(Clone)]
#[derive(Serialize, Deserialize)]
enum ParallelType {
    #[serde(rename = "per-block")]
    PerBlock,
    #[serde(rename = "per-frame")]
    PerFrame,
}

fn render_job_fields() -> serde_json::Value {
    serde_json::json!([
        ["total_frames", "integer"],
        ["samples_per_pixel", "integer"],
        ["width", "integer"],
        ["height", "integer"],
        ["parallel", ["per-block", "per-frame"]],
    ])
}

impl Default for RenderJob {
    fn default() -> Self {
        Self {
            total_frames: 40,
            samples_per_pixel: 128/4,
            width: 1280/4,
            height: 720/4,
            parallel: ParallelType::PerFrame,
        }
    }
}

struct RenderFrame {
    img: image::RgbImage,
    png: Vec<u8>,
}

struct RenderStatus {
    job: RenderJob,
    frames: Vec<(usize, RenderFrame)>,
    gif: Option<Vec<u8>>,
}

impl Default for RenderStatus {
    fn default() -> Self {
        Self {
            job: Default::default(),
            frames: vec![],
            gif: None,
        }
    }
}

#[derive(Debug)]
enum ClientState {
    NeedsConfig,
    NeedsFrameMeta(usize),
    NeedsFrame(usize),
    NeedsGifMeta,
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
    Meta(MetaMsg),
    Binary(Vec<u8>),
}

enum MetaMsg {
    Frame { index: usize },
    Gif,
    Reset(RenderJob, PoolStatus),
}

type PoolStatus = String;

impl Message for MyMsg {
    type Result = ();
}

impl Handler<MyMsg> for MyWs {
    type Result = ();

    fn handle(&mut self, msg: MyMsg, ctx: &mut Self::Context) {
        match msg {
            MyMsg::Binary(d) => ctx.binary(d),
            MyMsg::Meta(MetaMsg::Reset(job, pool_status)) =>
                ctx.text(serde_json::json!({
                    "job": job,
                    "job_fields": render_job_fields(),
                    "pool_status": pool_status,
                }).to_string()),
            MyMsg::Meta(MetaMsg::Frame { index }) =>
                ctx.text(serde_json::json!({
                    "frame": index,
                }).to_string()),
            MyMsg::Meta(MetaMsg::Gif) =>
                ctx.text(serde_json::json!({
                    "gif": null,
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

pub fn main(addr: String, cpus: usize) {
    let (job_tx, job_rx) = crossbeam::channel::unbounded();
    job_tx.send(Default::default()).unwrap(); // Reset to a valid job

    let state = MyServerData {
        inner: Arc::new(Mutex::new(
            MyServerDataInner {
                clients: HashMap::new(),
                job_tx,
                render: Default::default(),
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
    let ref should_stop_bool = AtomicBool::new(false);
    let should_stop = || should_stop_bool.load(Ordering::SeqCst);
    let set_stop = || should_stop_bool.store(true, Ordering::SeqCst);
    let pool = parallel::default_pool(cpus);
    let pool = &pool;
    crossbeam::scope(move |scope| {
        scope.spawn(move |scope| {

            let mut scene = one_weekend_scene();
            scene.build_bvh();

            let mut frame_rx = None;
            let never = crossbeam::channel::never();

            loop {
                if should_stop() {
                    println!("stopping rendering");
                    return
                }

                // Drain any incoming jobs
                loop {
                    match job_rx.try_recv() {
                        Ok(job) => {
                            frame_rx = Some(reset_job(job, &scene, &mut thread_state.lock(), scope, pool));
                        },
                        Err(crossbeam::channel::TryRecvError::Empty) => break,
                        Err(crossbeam::channel::TryRecvError::Disconnected) => {
                            println!("ERROR channel for receiving jobs closed");
                            return
                        },
                    }
                }

                crossbeam::channel::select! {
                    // New job arrived, attend to it
                    recv(job_rx) -> msg => {
                        match msg {
                            Ok(job) => {
                                frame_rx = Some(reset_job(job, &scene, &mut thread_state.lock(), scope, pool));
                            },
                            Err(crossbeam::channel::RecvError) => {
                                println!("ERROR channel for receiving jobs closed");
                                return
                            },
                        }
                    },
                    // New frame arrived, process it
                    recv(frame_rx.as_ref().unwrap_or(&never)) -> msg => {
                        match msg {
                            Ok((idx, w, h, raw)) => {
                                let img = image::RgbImage::from_raw(w, h, raw).unwrap();

                                let mut png = vec![];
                                let thumb = image::DynamicImage::ImageRgb8(image::imageops::thumbnail(&img, THUMB_MAX_PX, THUMB_MAX_PX));
                                thumb.write_to(&mut png, image::ImageOutputFormat::Png).unwrap();
                                println!("finished creating a png");

                                thread_state.lock().render.frames.push((idx, RenderFrame { img, png }));
                            },
                            Err(crossbeam::channel::RecvError) => {
                                println!("finished receiving frames");
                                frame_rx = None
                            },
                        }
                    }
                    // Timeout to attend to existing clients
                    default(Duration::from_millis(100)) => (),
                }

                // Update all connected clients
                thread_state.with(|ts| update_clients(ts, pool.status()));

                let needs_gif = thread_state.with(|s| (
                    s.render.frames.len() == s.render.job.total_frames && s.render.gif.is_none()
                ));

                // TODO: move this to a different thread. For now, it's below update_clients
                // so that the last frame that comes in gets sent out before we block on creating
                // the gif. Once it's on a different thread, move this back above update_clients
                if needs_gif {
                    // We've finished all frames, create the gif
                    thread_state.with(render_gif);
                    println!("finished creating a gif");
                }

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

        set_stop();
    }).unwrap();
}

fn reset_job<'a, 'b>(job: RenderJob, scene: &Scene, state: &mut MyServerDataInner, scope: &crossbeam::thread::Scope<'a>, pool: &'a impl ParallelExecutor) -> crossbeam::channel::Receiver<(usize, u32, u32, Vec<u8>)> {
    let (frame_tx, frame_rx) = crossbeam::channel::unbounded();
    let scene = scene.clone();
    match job.parallel {
        ParallelType::PerBlock => {
            let job = job.clone();
            scope.spawn(move |_| {
                for idx in 0..job.total_frames {
                    let render_worker = make_renderer(idx, scene.clone(), job.clone());
                    let img = futures::executor::block_on(render_frame_parallel(render_worker, pool));
                    println!("finished rendering a frame");
                    match frame_tx.send((idx, img.width(), img.height(), img.into_raw())) {
                        Ok(()) => (),
                        Err(crossbeam::channel::SendError(_)) => {
                            println!("terminating a processing thread as frame channel has closed");
                            return
                        },
                    }
                }
            });
        },
        ParallelType::PerFrame => {
            let job = job.clone();
            scope.spawn(move |_| {
                let mut futs: futures::stream::FuturesUnordered<_> = (0..job.total_frames)
                    .map(|idx| {
                        let render_worker = make_renderer(idx, scene.clone(), job.clone());
                        render_frame(render_worker, pool).map(move |img| (idx, img))
                    })
                    .collect();
                futures::executor::block_on(async {
                    while let Some((idx, img)) = futs.next().await {
                        match frame_tx.send((idx, img.width(), img.height(), img.into_raw())) {
                            Ok(()) => (),
                            Err(crossbeam::channel::SendError(_)) => {
                                println!("terminating a processing thread as frame channel has closed");
                                return
                            },
                        }
                    }
                });
            });
        },
    }
    state.render = RenderStatus { job, frames: vec![], gif: None };
    // Reset clients to receive the new job config
    for (_, cs) in state.clients.iter_mut() {
        *cs = ClientState::NeedsConfig
    }
    frame_rx
}

fn make_renderer(idx: usize, scene: Scene, job: RenderJob) -> render::Renderer {
    let delta_increment = PAN_RANGE / job.total_frames as f32;
    let delta_mult = (-(job.total_frames as f32) * delta_increment / 2.) + (idx as f32 * delta_increment);
    let cam = one_weekend_cam_lookat(job.width.into(), job.height.into(), Point3::ZERO + (Point3::ONE * delta_mult));
    render::Renderer::new(job.width.into(), job.height.into(), job.samples_per_pixel, scene, cam)
}

fn render_frame(render_worker: render::Renderer, pool: &impl ParallelExecutor) -> impl Future<Output=image::RgbImage> {
    render_worker.render_frame_single(pool)
}

fn render_frame_parallel(render_worker: render::Renderer, pool: &impl ParallelExecutor) -> impl Future<Output=image::RgbImage> {
    let img = image::RgbImage::new(render_worker.width(), render_worker.height());
    render_worker.render_frame_parallel(pool).fold(img, |mut img, (renderblock, result_img)| {
        img.copy_from(&result_img, renderblock.x, renderblock.y).unwrap();
        future::ready(img)
    })
}

fn render_gif(state: &mut MyServerDataInner) {
    let mut gif = vec![];
    let mut encoder = image::codecs::gif::GifEncoder::new(&mut gif);
    encoder.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
    let mut raws: Vec<_> = state.render.frames.iter().map(|(idx, frame)| (idx, frame.img.as_raw())).collect();
    raws.sort_by_key(|(idx, _)| *idx);
    for (_, img_raw) in raws {
        encoder.encode(img_raw, state.render.job.width.into(), state.render.job.height.into(), image::ColorType::Rgb8).unwrap();
    }
    drop(encoder);
    state.render.gif = Some(gif);
}

fn update_clients(state: &mut MyServerDataInner, pool_status: String) {
    for (addr, cs) in state.clients.iter_mut() {
        update_client(addr, cs, &state.render, &pool_status);
    }
}

fn update_client(addr: &Addr<MyWs>, cs: &mut ClientState, render: &RenderStatus, pool_status: &str) {
    loop {
        let (msg, next_cs) = match *cs {
            // Send the config
            ClientState::NeedsConfig => (MyMsg::Meta(MetaMsg::Reset(render.job.clone(), pool_status.to_owned())), ClientState::NeedsFrameMeta(0)),
            // Wants more frames, but the frames are finished - move onto the gif
            ClientState::NeedsFrameMeta(i) if i == render.job.total_frames => {
                *cs = ClientState::NeedsGifMeta;
                continue
            },
            // Wants more frames, but nothing to send yet
            ClientState::NeedsFrameMeta(i) if i == render.frames.len() => break,
            // Send a frame
            ClientState::NeedsFrame(i) => (MyMsg::Binary(render.frames[i].1.png.clone()), ClientState::NeedsFrameMeta(i+1)),
            ClientState::NeedsGif => {
                match render.gif.as_ref() {
                    // Send the gif
                    Some(gif) => (MyMsg::Binary(gif.clone()), ClientState::Complete),
                    // No gif available yet
                    None => break,
                }
            },
            // If needs some meta, send it and move to the actual data
            ClientState::NeedsFrameMeta(i) => (MyMsg::Meta(MetaMsg::Frame { index: render.frames[i].0 }), ClientState::NeedsFrame(i)),
            ClientState::NeedsGifMeta => (MyMsg::Meta(MetaMsg::Gif), ClientState::NeedsGif),
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
