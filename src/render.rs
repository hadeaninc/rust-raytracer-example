use futures::prelude::*;
use rand::Rng;
use serde::{Serialize, Deserialize};
use spiral::ChebyshevIterator;

use crate::camera::Camera;
use crate::parallel::ParallelExecutor;
use crate::scene::Scene;
use crate::shared::{TRACE_EPSILON, TRACE_INFINITY, Color, Ray, RayQuery, ceil_div, rgb_from_render};

const BLOCK_SIZE: u32 = 32;

/// Coordinates for a block to render
#[derive(Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RenderBlock {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Generates blocks of up to width,height for an image of width,height
pub struct ImageBlocker {
    pub image_width: u32,
    pub image_height: u32,
    pub block_width: u32,
    pub block_height: u32,
    pub block_count_x: u32,
    pub block_count_y: u32,
    block_index: u32,
}

impl ImageBlocker {
    fn new(image_width: u32, image_height: u32) -> Self {
        let block_width = BLOCK_SIZE;
        let block_height = BLOCK_SIZE;
        ImageBlocker {
            image_width: image_width,
            image_height: image_height,
            block_width: block_width,
            block_height: block_height,
            block_count_x: ceil_div(image_width, block_width),
            block_count_y: ceil_div(image_height, block_height),
            block_index: 0,
        }
    }
}

/// Iterator which generates a series of RenderBlock for the image
impl Iterator for ImageBlocker {
    type Item = RenderBlock;

    fn next(&mut self) -> Option<RenderBlock> {
        let block_count = self.block_count_x * self.block_count_y;

        if self.block_index >= block_count {
            return None;
        }

        let block_x = self.block_index % self.block_count_x;
        let block_y = self.block_index / self.block_count_x;

        let x = block_x * self.block_width;
        let y = block_y * self.block_width;
        let x_end = std::cmp::min((block_x + 1) * self.block_width, self.image_width);
        let y_end = std::cmp::min((block_y + 1) * self.block_height, self.image_height);

        let rb = RenderBlock {
            x: x,
            y: y,
            width: x_end - x,
            height: y_end - y,
        };

        self.block_index += 1;

        return Some(rb);
    }
}

/// Recursive ray tracing
fn ray_color(ray: Ray, scene: &Scene, depth: i32) -> Color {
    if depth <= 0 {
        return Color::ZERO;
    }

    // Intersect scene
    let query = RayQuery {
        ray: ray,
        t_min: TRACE_EPSILON,
        t_max: TRACE_INFINITY,
    };
    let hit_option = scene.intersect(query);

    // If we hit something
    if let Some(hit) = hit_option {
        let scatter_option = hit.material.scatter(&ray, &hit);

        // Recurse
        if let Some(scatter) = scatter_option {
            return scatter.attenuation
                * ray_color(scatter.scattered_ray, scene, depth - 1);
        }

        return Color::ZERO;
    }

    // Background
    let unit_direction = ray.direction.normalize();
    let t = 0.5 * (unit_direction.y + 1.0);
    return (1.0 - t) * Color::new(1.0, 1.0, 1.0) + t * Color::new(0.5, 0.7, 1.0);
}

/// Renderer which generates pixels using the scene and camera, and returns them via a stream
pub struct Renderer {
    image_width: u32,
    image_height: u32,
    scene: Scene,
    camera: Camera,
    samples_per_pixel: u32,
    max_depth: i32,
}

impl Renderer {
    pub fn new(
        image_width: u32,
        image_height: u32,
        samples_per_pixel: u32,
        scene: Scene,
        camera: Camera,
    ) -> Self {
        Renderer {
            image_width: image_width,
            image_height: image_height,
            scene: scene,
            camera: camera,
            samples_per_pixel: samples_per_pixel,
            max_depth: 50,
        }
    }

    pub fn render_frame_parallel(self, pool: &mut impl ParallelExecutor) -> impl Stream<Item=(RenderBlock, image::RgbImage)> {
        // Generate blocks to render the image
        let blocker = ImageBlocker::new(self.image_width, self.image_height);
        let block_count_x = blocker.block_count_x as i32;
        let block_count_y = blocker.block_count_y as i32;
        let blocks: Vec<RenderBlock> = blocker.collect();

        // Set up ChebyshevIterator. A bit awkward because it is square and generates out of bound XY which we need to check.
        let radius = ((std::cmp::max(block_count_x, block_count_y) / 2) + 1) as u16;
        let center_x = block_count_x / 2 - 1;
        let center_y = block_count_y / 2 - 1;
        let mut spiral_blocks = Vec::new();

        // Loop blocks in spiral order using ChebyshevIterator
        for (block_x, block_y) in ChebyshevIterator::new(center_x, center_y, radius) {
            if block_x < 0 || block_x >= block_count_x || block_y < 0 || block_y >= block_count_y {
                continue; // Block out of bounds, ignore.
            }
            let block_index = (block_y * block_count_x + block_x) as usize;
            spiral_blocks.push(blocks[block_index])
        }

        // Loop blocks in the image blocker and spawn renderblock tasks
        let mut futs = futures::stream::FuturesOrdered::new();
        for renderblock in spiral_blocks {
            futs.push(
                pool.execute(render_block, Ctx {
                    renderblock,
                    image_width: self.image_width,
                    image_height: self.image_height,
                    scene: self.scene.clone(),
                    camera: self.camera.clone(),
                    samples_per_pixel: self.samples_per_pixel,
                    max_depth: self.max_depth,
                }).map(move |image|
                    (renderblock, image::RgbImage::from_raw(renderblock.width, renderblock.height, image).unwrap())
                )
            );
        }

        futs
    }
}

#[derive(Serialize, Deserialize)]
struct Ctx {
    renderblock: RenderBlock,
    image_width: u32,
    image_height: u32,
    scene: Scene,
    camera: Camera,
    samples_per_pixel: u32,
    max_depth: i32,
}

fn render_block(Ctx { renderblock, image_width, image_height, scene, camera, samples_per_pixel, max_depth }: Ctx) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    let mut img = image::RgbImage::new(renderblock.width, renderblock.height);
    img.enumerate_pixels_mut().for_each(|(px, py, pixel)| {
        // Compute pixel location
        let x = renderblock.x + px;
        let y = renderblock.y + py;

        // Set up supersampling
        let mut color_accum = Color::ZERO;
        let u_base = x as f32 / (image_width as f32 - 1.0);
        let v_base = (image_height - y - 1) as f32
            / (image_height as f32 - 1.0);
        let u_rand = 1.0 / (image_width as f32 - 1.0);
        let v_rand = 1.0 / (image_height as f32 - 1.0);

        // Supersample this pixel
        for _ in 0..samples_per_pixel {
            let u = u_base + rng.gen_range(0.0..u_rand);
            let v = v_base + rng.gen_range(0.0..v_rand);
            let ray = camera.get_ray(u, v);
            // Start the primary here from here
            color_accum += ray_color(ray, &scene, max_depth);
        }
        color_accum /= samples_per_pixel as f32;

        *pixel = rgb_from_render(color_accum);
    });
    img.into_raw()
}
