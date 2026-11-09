// SPDX-License-Identifier: GPL-3.0-only

//! The grain over a panel's tint: a faint white noise that breaks up the
//! soft shapes of whatever the compositor's blur leaves of the text behind.
//! Drawn by a small shader, one speck per device pixel at any size: no
//! picture to bake, nothing to scale.

use std::sync::atomic::{AtomicUsize, Ordering};

use cosmic::{
    Element,
    iced::{
        Length, Rectangle,
        wgpu,
        widget::shader::{self, Viewport},
    },
};

/// How strong the grain is at most; half that on average.
const STRENGTH: f32 = 0.09;

/// Grain over the rounded panel this widget covers.
pub fn view<'a, M: 'a>(radius: f32) -> Element<'a, M> {
    shader::Shader::new(Grain { radius }).width(Length::Fill).height(Length::Fill).into()
}

struct Grain {
    radius: f32,
}

impl<M> shader::Program<M> for Grain {
    type State = ();
    type Primitive = Primitive;

    fn draw(&self, _state: &(), _cursor: cosmic::iced::mouse::Cursor, bounds: Rectangle) -> Primitive {
        Primitive { bounds, radius: self.radius, slot: AtomicUsize::new(0) }
    }
}

/// One panel's grain: where it is, how round, and which uniform slot it
/// was given this frame.
#[derive(Debug)]
pub struct Primitive {
    bounds: Rectangle,
    radius: f32,
    slot: AtomicUsize,
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(&self, pipeline: &mut Pipeline, device: &wgpu::Device, queue: &wgpu::Queue, _bounds: &Rectangle, viewport: &Viewport) {
        let scale = viewport.scale_factor() as f32;
        let uniforms = Uniforms {
            // The panel in device pixels, as the fragment sees its own position.
            rect: [self.bounds.x * scale, self.bounds.y * scale, self.bounds.width * scale, self.bounds.height * scale],
            radius: self.radius * scale,
            strength: STRENGTH,
            _pad: [0.0; 2],
        };
        let slot = pipeline.slot(device, queue, &uniforms);
        self.slot.store(slot, Ordering::Relaxed);
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(slot) = pipeline.slots.get(self.slot.load(Ordering::Relaxed)) else {
            return false;
        };
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &slot.bind_group, &[]);
        // One triangle over the whole viewport; the fragment does the rest.
        pass.draw(0..3, 0..1);
        true
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Uniforms {
    rect: [f32; 4],
    radius: f32,
    strength: f32,
    _pad: [f32; 2],
}

impl Uniforms {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32);
        for value in self.rect.iter().chain([self.radius, self.strength].iter()).chain(self._pad.iter()) {
            out.extend_from_slice(&value.to_ne_bytes());
        }
        out
    }
}

struct Slot {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// The one pipeline all panels share, with a uniform slot per panel drawn
/// this frame; the slots are handed out again next frame.
pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    slots: Vec<Slot>,
    used: usize,
}

impl Pipeline {
    fn slot(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, uniforms: &Uniforms) -> usize {
        let index = self.used;
        self.used += 1;
        if index >= self.slots.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cabin grain uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cabin grain"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.slots.push(Slot { buffer, bind_group });
        }
        queue.write_buffer(&self.slots[index].buffer, 0, &uniforms.bytes());
        index
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cabin grain"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cabin grain"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cabin grain"),
            bind_group_layouts: &[&layout],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cabin grain"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, layout, slots: Vec::new(), used: 0 }
    }

    fn trim(&mut self) {
        self.used = 0;
    }
}

/// A triangle over the viewport; each fragment hashes its own device
/// position into a speck, within the panel's rounded rectangle.
const SHADER: &str = r#"
struct Uniforms {
    rect: vec4<f32>,
    radius: f32,
    strength: f32,
    pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // Three corners well past the viewport: the clip takes care of the rest.
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = position.xy;
    // Signed distance to the rounded rectangle: outside it, nothing.
    let half = u.rect.zw * 0.5;
    let center = u.rect.xy + half;
    let q = abs(p - center) - half + vec2<f32>(u.radius, u.radius);
    let distance = length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - u.radius;
    let inside = clamp(0.5 - distance, 0.0, 1.0);
    let alpha = hash(floor(p)) * u.strength * inside;
    return vec4<f32>(alpha, alpha, alpha, alpha);
}
"#;
