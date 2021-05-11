use crate::object::*;
use crate::shared::*;

use bvh::bvh::BVH;

/// Basic scene which holds objects and a BVH
pub struct Scene {
    // List of generic hittables
    pub objects_other: Vec<Box<dyn RayHittable>>,

    // List of spheres
    pub objects_sphere: Vec<Sphere>,

    // List of bounds for hittables
    pub bounds: Vec<HittableBounds>,

    // Acceleration structure
    pub bvh: Option<BVH>,
}

impl Scene {
    pub fn new() -> Self {
        Scene {
            objects_other: Vec::new(),
            objects_sphere: Vec::new(),
            bounds: Vec::new(),
            bvh: None,
        }
    }

    pub fn add_sphere(&mut self, s: Sphere)
    {
        self.objects_sphere.push(s);
    }

    pub fn build_bvh(&mut self) {
        // Compute bounds
        for (i, hittable) in self.objects_other.iter().enumerate() {
            self.bounds.push(hittable.compute_bounds(i));
        }
        for (i, sphere) in self.objects_sphere.iter().enumerate() {
            self.bounds.push(sphere.compute_bounds(i));
        }
        // Build BVH
        self.bvh = Some(BVH::build(&mut self.bounds));
    }

    /// Return the closest intersection (or None) in the scene using the ray
    pub fn intersect(&self, mut query: RayQuery) -> Option<HitRecord> {
        let mut closest_hit_option: Option<HitRecord> = None;

        if let Some(bvh) = &self.bvh {
            // Traverse the BVH
            let bvh_ray = bvh::ray::Ray::new(query.ray.origin, query.ray.direction);
            let hit_bounds = bvh.traverse_iterator(&bvh_ray, &self.bounds);

            // Iterate over hit objects to find closest
            for bounds in hit_bounds {
                let obj = &self.objects_sphere[bounds.hittable_index];
                let hit_option = obj.intersect(query);
                if hit_option.is_some() {
                    // Shorten the ray
                    query.t_max = f32::min(query.t_max, hit_option.as_ref().unwrap().t);
                }
                if closest_hit_option.is_none() {
                    closest_hit_option = hit_option;
                } else if hit_option.is_some() {
                    let closest_hit = closest_hit_option.as_ref().unwrap();
                    let hit = hit_option.as_ref().unwrap();
                    if hit.t < closest_hit.t {
                        closest_hit_option = hit_option;
                    }
                }
            }
        }
        return closest_hit_option;
    }

    pub fn intersect_packet(&self, packet: &RayPacket) -> [Option<HitRecord>; TRACE_PACKET_SIZE] {
        <[Option<HitRecord>; TRACE_PACKET_SIZE]>::init_with_indices(|i| {
            let ray = packet.rays[i];
            let query = RayQuery {
                ray: ray,
                t_min: TRACE_EPSILON,
                t_max: TRACE_INFINITY,
            };
            self.intersect(query)
        })
    }
}
