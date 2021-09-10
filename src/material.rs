use rand::Rng;
use serde::{Serialize, Deserialize};

use crate::object::HitRecord;
use crate::shared::{Color, Ray, Vec3, VecExt, random_in_unit_sphere, reflectance, random_unit_vector, vec_reflect, vec_refract};

/// A material which can scatter rays
#[derive(Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub enum Material {
    Lambertian(Lambertian),
    Metal(Metal),
    Dielectric(Dielectric),
}

impl Material {
    pub fn scatter(&self, ray: &Ray, hit: &HitRecord) -> Option<ScatterResult> {
        match self {
            Material::Lambertian(m) => m.scatter(ray, hit),
            Material::Metal(m) => m.scatter(ray, hit),
            Material::Dielectric(m) => m.scatter(ray, hit),
        }
    }
}

/// Result of Material::scatter
pub struct ScatterResult {
    pub attenuation: Color,
    pub scattered_ray: Ray,
}

#[derive(Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct Lambertian {
    pub albedo: Color,
}

impl Lambertian {
    fn scatter(&self, _ray: &Ray, hit: &HitRecord) -> Option<ScatterResult> {
        let mut scatter_direction = hit.normal + random_unit_vector();
        if scatter_direction.near_zero() {
            scatter_direction = hit.normal;
        }

        let scattered = Ray::new(hit.point, scatter_direction);
        Some(ScatterResult {
            attenuation: self.albedo,
            scattered_ray: scattered,
        })
    }
}

#[derive(Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct Metal {
    pub albedo: Color,
    pub fuzz: f32,
}

impl Metal {
    fn scatter(&self, ray: &Ray, hit: &HitRecord) -> Option<ScatterResult> {
        let reflected = vec_reflect(ray.direction.normalize(), hit.normal);

        let scattered = Ray::new(hit.point, reflected + self.fuzz * random_in_unit_sphere());
        Some(ScatterResult {
            attenuation: self.albedo,
            scattered_ray: scattered,
        })
    }
}

#[derive(Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct Dielectric {
    pub ir: f32,
}

impl Dielectric {
    fn scatter(&self, ray: &Ray, hit: &HitRecord) -> Option<ScatterResult> {
        let mut rng = rand::thread_rng();

        let attenuation = Color::new(1.0, 1.0, 1.0);
        let refraction_ratio = if hit.front_face {
            1.0 / self.ir
        } else {
            self.ir
        };

        let unit_direction = ray.direction.normalize();
        let cos_theta = f32::min((-unit_direction).dot(hit.normal), 1.0);
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();

        let cannot_refract = refraction_ratio * sin_theta > 1.0;
        let direction: Vec3;
        if cannot_refract || reflectance(cos_theta, refraction_ratio) > rng.gen_range(0.0..1.0) {
            direction = vec_reflect(unit_direction, hit.normal);
        } else {
            direction = vec_refract(unit_direction, hit.normal, refraction_ratio);
        }

        let scattered = Ray::new(hit.point, direction);
        Some(ScatterResult {
            attenuation: attenuation,
            scattered_ray: scattered,
        })
    }
}
