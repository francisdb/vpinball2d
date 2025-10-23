//! Creating polygons from a list of vertices

// TODO have a look at https://github.com/bevy-procedural/modelling

use bevy::prelude::*;

pub(crate) fn triangulate_polygon(vertices: &[Vec2]) -> Vec<u32> {
    if vertices.len() < 3 {
        return vec![];
    }

    let mut indices = Vec::new();

    // Create a mutable array of available vertices
    let mut remaining: Vec<usize> = (0..vertices.len()).collect();

    // Continue until we've used all vertices except the last 2
    let mut attempts = 0;
    let max_attempts = vertices.len() * vertices.len(); // Safety limit

    while remaining.len() > 2 && attempts < max_attempts {
        let n = remaining.len();

        // Find an ear
        for i in 0..n {
            let prev = (i + n - 1) % n;
            let curr = i;
            let next = (i + 1) % n;

            let prev_idx = remaining[prev];
            let curr_idx = remaining[curr];
            let next_idx = remaining[next];

            // Check if vertex forms an ear (internal angle < 180°)
            if is_ear(vertices, &remaining, prev, curr, next) {
                // Add triangle
                indices.push(prev_idx as u32);
                indices.push(curr_idx as u32);
                indices.push(next_idx as u32);

                // Remove the ear tip from remaining vertices
                remaining.remove(curr);
                break;
            }
        }

        attempts += 1;
    }
    indices
}

fn is_ear(vertices: &[Vec2], remaining: &[usize], prev: usize, curr: usize, next: usize) -> bool {
    let prev_idx = remaining[prev];
    let curr_idx = remaining[curr];
    let next_idx = remaining[next];

    let p0 = vertices[prev_idx];
    let p1 = vertices[curr_idx];
    let p2 = vertices[next_idx];

    // First, check if this is a convex corner
    if !is_convex(p0, p1, p2) {
        return false;
    }

    // Then check if any remaining vertex is inside this triangle
    for &i in remaining {
        if i == prev_idx || i == curr_idx || i == next_idx {
            continue;
        }

        if point_in_triangle(vertices[i], p0, p1, p2) {
            return false;
        }
    }

    true
}

fn is_convex(p0: Vec2, p1: Vec2, p2: Vec2) -> bool {
    // Calculate the cross product to determine convexity
    let v1 = Vec2::new(p1.x - p0.x, p1.y - p0.y);
    let v2 = Vec2::new(p2.x - p1.x, p2.y - p1.y);
    let cross = v1.x * v2.y - v1.y * v2.x;

    // Positive cross product means counter-clockwise, which is what we want
    cross > 0.0
}

fn point_in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    // Barycentric coordinate method
    let area = 0.5 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y)).abs();

    // Calculate areas of three triangles made by point p and vertices of the triangle
    let alpha = 0.5 * ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / area;
    let beta = 0.5 * ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / area;
    let gamma = 1.0 - alpha - beta;

    // If all coordinates are between 0 and 1, point is inside triangle
    alpha >= 0.0 && beta >= 0.0 && gamma >= 0.0
}
