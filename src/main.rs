mod camera;
mod material;
mod object;
mod render;
mod scene;
mod server;
mod shared;

use std::path::PathBuf;
use rand::{Rng, SeedableRng};

use camera::Camera;
use material::{Dielectric, Lambertian, Material, Metal};
use object::Sphere;
use scene::Scene;
use shared::{Color, Point3, Vec3, color_random, color_random_range};
use structopt::StructOpt;

mod parallel {
    use futures::executor::ThreadPool;
    use futures::task::SpawnExt;
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use std::future::Future;
    use std::pin::Pin;

    #[cfg(feature = "distributed")]
    use hadean::pool::HadeanPool;

    // TODO: I think we want ATCs here to be able to use Future as an associated type and then use it
    // in the return type of execute
    pub trait ParallelExecutor: Send + Sync {
        fn execute<
            T: Serialize + DeserializeOwned + Send + Unpin + 'static,
            R: Serialize + DeserializeOwned + Send + Unpin + 'static,
        >(&self, f: fn(T) -> R, ctx: T) -> Pin<Box<dyn Future<Output=R>>>;
        fn status(&self) -> String;
    }

    impl ParallelExecutor for ThreadPool {
        fn execute<
            T: Serialize + DeserializeOwned + Send + Unpin + 'static,
            R: Serialize + DeserializeOwned + Send + Unpin + 'static,
        >(&self, f: fn(T) -> R, ctx: T) -> Pin<Box<dyn Future<Output=R>>> {
            Box::pin(self.spawn_with_handle(futures::future::lazy(move |_| f(ctx))).unwrap())
        }
        fn status(&self) -> String {
            "[running threads]".into()
        }
    }

    #[cfg(feature = "distributed")]
    impl ParallelExecutor for HadeanPool {
        fn execute<
            T: Serialize + DeserializeOwned + Send + Unpin + 'static,
            R: Serialize + DeserializeOwned + Send + Unpin + 'static,
        // TODO: if I can make this a shared ref then make the trait shared ref too
        >(&self, f: fn(T) -> R, ctx: T) -> Pin<Box<dyn Future<Output=R>>> {
            Box::pin(HadeanPool::execute(self, f, ctx))
        }
        fn status(&self) -> String {
            let status = self.status();
            format!("{:?}", status)
        }
    }

    #[cfg(feature = "distributed")]
    pub fn default_pool(cores: usize) -> impl ParallelExecutor {
        HadeanPool::new(cores)
    }
    #[cfg(not(feature = "distributed"))]
    pub fn default_pool(cores: usize) -> impl ParallelExecutor {
        ThreadPool::builder().pool_size(cores).create().unwrap()
    }
}

fn one_weekend_cam(width: usize, height: usize) -> Camera {
    one_weekend_cam_lookat(width, height, Point3::new(0.0, 0.0, 0.0))
}

fn one_weekend_cam_lookat(width: usize, height: usize, lookat: Point3) -> Camera {
    let aspect_ratio = (width as f32) / (height as f32);

    let lookfrom = Point3::new(13.0, 2.0, 3.0);
    let vup = Vec3::new(0.0, 1.0, 0.0);
    let dist_to_focus = 10.0;
    let aperture = 0.1;

    Camera::new(
        lookfrom,
        lookat,
        vup,
        20.0,
        aspect_ratio,
        aperture,
        dist_to_focus,
    )
}

/// Generate the ray tracing in one weekend scene
fn one_weekend_scene() -> Scene {
    let mut rng = rand_pcg::Pcg32::seed_from_u64(2);
    let mut scene = Scene::new();

    let mut spheres: Vec<(Point3, f32)> = Vec::new();
    let mut add_sphere =
        |spheres: &mut Vec<(Point3, f32)>, c: Point3, r: f32, mat: Material| {
            scene.objects.push(Sphere::new(c, r, mat.clone()));
            spheres.push((c, r));
        };

    let sphere_intersects = |spheres: &Vec<(Point3, f32)>, c: Point3, r: f32| {
        spheres.iter().any(|s| (s.0 - c).length() < (s.1 + r))
    };

    let ground_material: Material = Material::Lambertian(Lambertian {
        albedo: Color::new(0.5, 0.5, 0.5),
    });
    add_sphere(
        &mut spheres,
        Point3::new(0.0, -1000.0, -1.0),
        1000.0,
        ground_material,
    );

    let material1: Material = Material::Dielectric(Dielectric { ir: 1.5 });
    add_sphere(&mut spheres, Point3::new(0.0, 1.0, 0.0), 1.0, material1);

    let material2: Material = Material::Lambertian(Lambertian {
        albedo: Color::new(0.4, 0.2, 0.1),
    });
    add_sphere(&mut spheres, Point3::new(-4.0, 1.0, 0.0), 1.0, material2);

    let material3: Material = Material::Metal(Metal {
        albedo: Color::new(0.7, 0.6, 0.5),
        fuzz: 0.0,
    });
    add_sphere(&mut spheres, Point3::new(4.0, 1.0, 0.0), 1.0, material3);

    for a in -11..11 {
        for b in -11..11 {
            let choose_mat = rng.gen_range(0.0..1.0);
            let mut center;
            // Find a position which doesn't intersect with any other sphere
            loop {
                center = Point3::new(
                    a as f32 + 0.9 * rng.gen_range(0.0..1.0),
                    0.2,
                    b as f32 + 0.9 * rng.gen_range(0.0..1.0),
                );
                if !sphere_intersects(&spheres, center, 0.2) {
                    break;
                }
            }

            if (center - Point3::new(4.0, 0.2, 0.0)).length() > 0.9 {
                if choose_mat < 0.7 {
                    // diffuse
                    let albedo = color_random(&mut rng);
                    let sphere_material: Material =
                        Material::Lambertian(Lambertian { albedo: albedo });
                    add_sphere(&mut spheres, center, 0.2, sphere_material);
                } else if choose_mat < 0.95 {
                    // metal
                    let albedo = color_random_range(&mut rng, 0.5, 1.0);
                    let fuzz = rng.gen_range(0.0..0.5);
                    let sphere_material: Material = Material::Metal(Metal {
                        albedo: albedo,
                        fuzz: fuzz,
                    });
                    add_sphere(&mut spheres, center, 0.2, sphere_material);
                } else {
                    // glass
                    let sphere_material: Material = Material::Dielectric(Dielectric { ir: 1.5 });
                    add_sphere(&mut spheres, center, 0.2, sphere_material);
                }
            }
        }
    }

    return scene;
}

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, StructOpt)]
enum Cmd {
    #[structopt(about = "start a web server with a rendering control panel on 0.0.0.0:28888")]
    Serve {
        cpus: Option<usize>,
    },
    #[structopt(about = "render an X11 window with a single frame being processed in parallel in blocks")]
    Window {
        #[structopt(long)]
        out_file: Option<PathBuf>,
        cpus: Option<usize>,
    },
    #[structopt(about = "perform some size analysis, useful for assessing how much data may move over the wire")]
    SizeAnalyze,
}

fn main() {
    std::env::set_var("RUST_BACKTRACE", "1"); // hack around hadean environment variables
    #[cfg(feature = "distributed")]
    {
        hadean::hadean::init();
    }

    let opt = Opt::from_args();

    match opt.cmd {
        Cmd::Serve { cpus } => {
            server::main("0.0.0.0:28888".to_owned(), cpus.unwrap_or_else(|| num_cpus::get() - 1))
        },
        Cmd::Window { cpus, out_file } => {
            window::main(out_file, cpus.unwrap_or_else(|| num_cpus::get() - 1))
        },
        Cmd::SizeAnalyze => {
            let width = 1280/4;
            let height = 720/4;
            let frames = 40;

            let block_size = 32;

            const CHANNELS: usize = 3;
            const UNIT_BOUND: usize = 1024;

            fn sizefmt(mut n: usize) -> String {
                if n < UNIT_BOUND {
                    return format!("{}B", n)
                }
                n /= 1024;
                if n < UNIT_BOUND {
                    return format!("{}KiB", n)
                }
                n /= 1024;
                return format!("{}MiB", n)
            }

            println!("# IMAGE");
            println!("Considering an image of {}px x {}px with {} frames", width, height, frames);
            let frame_bytes = width * height * CHANNELS;
            println!("{} for pixels for a single uncompressed frame", sizefmt(frame_bytes));
            println!("{} for all {} frames", sizefmt(frame_bytes * frames), frames);
            println!("");

            println!("# BLOCKS");
            let approx_num_blocks_per_frame = (width * height) / (block_size * block_size);
            let approx_block_overhead_per_frame = approx_num_blocks_per_frame * std::mem::size_of::<render::RenderBlock>();
            let approx_block_overhead_total = approx_block_overhead_per_frame * frames;
            println!("Processing with blocks adds approx {} per frame", sizefmt(approx_block_overhead_per_frame));
            println!("Processing with blocks adds {} for all {} frames",  sizefmt(approx_block_overhead_total), frames);
            println!("");

            println!("# JOB");
            let scene = one_weekend_scene();
            let cam = one_weekend_cam(width, height);
            let scene_bytes = bincode::serialized_size(&scene).unwrap() as usize;
            let cam_bytes = bincode::serialized_size(&cam).unwrap() as usize;
            let job_bytes = scene_bytes + cam_bytes;
            let pct_scene = 100. * scene_bytes as f32 / (scene_bytes + cam_bytes) as f32;
            println!("scene data is {} ({:.2}%) and cam data is {} ({:.2}%)", sizefmt(scene_bytes), pct_scene, sizefmt(cam_bytes), 100. - pct_scene);
            println!("sending job data per frame costs {} for all {} frames", sizefmt(job_bytes * frames), frames);
            println!("sending job data per block costs {} for all {} frames", sizefmt(job_bytes * frames * approx_num_blocks_per_frame), frames);
            println!("");

            println!("# PIXELRESULT (old way of transferring data)");
            struct PixelResult {
                _x: u32,
                _y: u32,
                _color: Color,
            }
            let pixelresult_frame_bytes = width * height * std::mem::size_of::<PixelResult>();
            println!("{} for a frame's worth of PixelResults", sizefmt(pixelresult_frame_bytes));
            println!("{} for all {} frames", sizefmt(pixelresult_frame_bytes * frames), frames);
            println!("");
        },
    }
}

#[cfg(not(feature = "gui"))]
mod window {
    use std::path::PathBuf;
    use std::process;

    pub fn main(_out_file: Option<PathBuf>, _cpus: usize) {
        println!("gui support not compiled in - please recompile with 'gui' feature");
        process::exit(1);
    }
}

#[cfg(feature = "gui")]
mod window {
    use futures::prelude::*;
    use minifb::{Key, Window, WindowOptions};
    use std::path::PathBuf;

    use crate::parallel;
    use crate::render;
    use crate::{one_weekend_cam, one_weekend_scene};

    type ColorDisplay = u32;

    fn u8_vec_from_buffer_display(bd: &[ColorDisplay]) -> Vec<u8> {
        bd
            .into_iter()
            .flat_map(|x| u8_vec_from_color_display(*x))
            .collect()
    }

    fn u8_vec_from_color_display(c: ColorDisplay) -> [u8; 3] {
        let b = c as u8;
        let g = (c >> 8) as u8;
        let r = (c >> 16) as u8;
        return [r, g, b];
    }

    fn color_display_from_rgb(rgb: image::Rgb<u8>) -> ColorDisplay {
        let (r, g, b) = (rgb[0] as u32, rgb[1] as u32, rgb[2] as u32);
        (r << 16) | (g << 8) | b
    }

    fn index_from_xy(image_width: u32, _image_height: u32, x: u32, y: u32) -> usize {
        (y * image_width + x) as usize
    }

    pub fn main(out_file: Option<&str>) {
        const WIDTH: usize = 1280;
        const HEIGHT: usize = 720;
        const SAMPLES_PER_PIXEL: u32 = 128;

        #[cfg(feature = "distributed")]
        std::env::set_var("DISPLAY", ":0"); // hack around hadean environment variables for local runs

        let mut window = Window::new(
            "Ray tracing in one weekend - ESC to exit",
            WIDTH,
            HEIGHT,
            WindowOptions::default(),
        )
        .unwrap_or_else(|e| {
            panic!("{}", e);
        });

        // Limit to max ~60 fps update rate
        window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

        let mut scene = one_weekend_scene();
        scene.build_bvh();
        let cam = one_weekend_cam(WIDTH, HEIGHT);

        let render_worker =
            render::Renderer::new(WIDTH as u32, HEIGHT as u32, SAMPLES_PER_PIXEL, scene, cam);

        let mut buffer_display = vec![0; WIDTH * HEIGHT];

        let mut pool = parallel::default_pool(cpus);

        crossbeam::scope(|scope| {
            let (tx, rx) = crossbeam::channel::unbounded();

            // TODO: ideally this shouldn't be a thread
            scope.spawn(move |_| {
                let mut stream = render_worker.render_frame_parallel(&mut pool);
                futures::executor::block_on(async {
                    while let Some(results) = stream.next().await {
                        match tx.send(results) {
                            Ok(()) => (),
                            Err(crossbeam::channel::SendError(_)) => break,
                        }
                    }
                })
            });

            while window.is_open() && !window.is_key_down(Key::Escape) {
                let has_changed = match rx.try_recv() {
                    Ok((renderblock, result_img)) => {
                        for (px, py, pixel) in result_img.enumerate_pixels() {
                            let index = index_from_xy(WIDTH as u32, HEIGHT as u32, renderblock.x + px, renderblock.y + py);
                            buffer_display[index] = color_display_from_rgb(*pixel);
                        }
                        true
                    },
                    Err(crossbeam::channel::TryRecvError::Empty) |
                    Err(crossbeam::channel::TryRecvError::Disconnected) => false,
                };
                if has_changed {
                    window
                        .update_with_buffer(&buffer_display, WIDTH, HEIGHT)
                        .unwrap();
                } else {
                    window.update();
                }
            }
        })
        .unwrap();

        // If we get one argument, assume it's our output png filename
        if let Some(out_file) = out_file {
            let pixels = u8_vec_from_buffer_display(&buffer_display);
            let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(WIDTH as u32, HEIGHT as u32, pixels).unwrap());
            img.save(out_file).unwrap();
        }
    }
}
