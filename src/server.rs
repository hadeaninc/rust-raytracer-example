use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, Running, StreamHandler};
use actix_web::{App, Error, HttpResponse, HttpRequest, HttpServer};
use actix_web::http::header::ContentEncoding;
use actix_web::middleware;
use actix_web::web;
use actix_web_actors::ws;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::parallel::{self, ParallelExecutor};
use crate::render;
use crate::shared::{ColorDisplay, Point3, color_display_from_render, index_from_xy};

use super::{one_weekend_cam_lookat, one_weekend_scene, write_png};

const WIDTH: usize = 1280/4;
const HEIGHT: usize = 720/4;
const SAMPLES_PER_PIXEL: u32 = 128/8;

#[derive(Clone)]
struct MyServerData {
    data: Arc<Mutex<HashMap<Addr<MyWs>, usize>>>,
}
type ServerData = web::Data<MyServerData>;

struct MyWs {
    state: MyServerData,
}

struct MyMsg(Vec<u8>);
impl Message for MyMsg {
    type Result = ();
}

impl Handler<MyMsg> for MyWs {
    type Result = ();

    fn handle(&mut self, msg: MyMsg, ctx: &mut Self::Context) {
        ctx.binary(msg.0);
    }
}

impl Actor for MyWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        println!("starting a websocket stream");
        let addr = ctx.address();
        let prev = self.state.data.lock().unwrap().insert(addr, 0);
        assert!(prev.is_none())
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        println!("stopping a websocket stream");
        let addr = ctx.address();
        let prev = self.state.data.lock().unwrap().remove(&addr);
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

pub fn main(addr: String) {
    let state = MyServerData { data: Arc::new(Mutex::new(HashMap::new())) };

    let app_state = state.clone();
    let app_factory = move || {
        let app = App::new();
        let app = app.data(app_state.clone());
        let app = app.wrap(middleware::Logger::default());
        let app = app.wrap(middleware::Compress::new(ContentEncoding::Auto));
        let app = app.route("/ws", web::get().to(ws));
        let app = app.service(actix_files::Files::new("/", "static").index_file("index.html"));
        app
    };

    let thread_state = state.clone();
    let ref should_stop = Arc::new(AtomicBool::new(false));
    crossbeam::scope(move |scope| {
        scope.spawn(move |_| {
            let mut pool = parallel::default_pool(num_cpus::get());
            let mut frames = vec![];

            let mut scene = one_weekend_scene();
            scene.build_bvh();
            let lookat = Point3::new(0.0, 0.0, 0.0);

            let mut deltas = vec![];
            let mut mult = -4.0;
            while mult < 4.0 {
                deltas.push(Point3::ONE * mult);
                mult += 0.5;
            }

            let mut delta_idx = 0;
            loop {
                if should_stop.load(Ordering::SeqCst) {
                    println!("stopping rendering");
                    return
                }

                let cam = one_weekend_cam_lookat(WIDTH, HEIGHT, lookat + deltas[delta_idx % deltas.len()]);
                let render_worker = render::Renderer::new(WIDTH as u32, HEIGHT as u32, SAMPLES_PER_PIXEL, scene.clone(), cam);
                delta_idx += 1;

                let buffer_display = render_and_return(&render_worker, &mut pool);
                println!("finished rendering a frame");
                let mut png = vec![];
                write_png(WIDTH, HEIGHT, &mut png, &buffer_display);
                println!("finished creating a png");
                frames.push(png);
                for (addr, i) in thread_state.data.lock().unwrap().iter_mut() {
                    while *i < frames.len() {
                        match addr.try_send(MyMsg(frames[*i].clone())) {
                            Ok(()) => *i += 1,
                            Err(e) => {
                                println!("failed to send: {}", e);
                                break
                            },
                        }
                    }
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

