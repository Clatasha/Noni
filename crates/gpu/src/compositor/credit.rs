use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use bv_effects::accel::{CreditLinePlan, CreditMask, CreditRoll, Value, Values};
use bv_media::Ask;
use bv_timeline::scene::MediaLayer;
use bv_timeline::{Generator, MediaId, MediaSource, PluginValue};

use super::{Compositor, Img};
use crate::kernels::{Grid, K, Params};

const CACHE_BYTES: u64 = 64 << 20;
type Key = (usize, usize, u8);

pub(super) struct Roll {
    id: MediaId,
    size: (u32, u32),
    plan: Option<CreditRoll>,
    masks: HashMap<Key, Mask>,
    oldest: VecDeque<Key>,
    bytes: u64,
}

#[derive(Clone)]
struct Mask {
    buffer: wgpu::Buffer,
    origin: [i64; 2],
    size: [u32; 2],
    color: [f32; 3],
}

impl Roll {
    fn remember(&mut self, key: Key, mask: Mask) {
        while self.bytes + mask.buffer.size() > CACHE_BYTES {
            let Some(old) = self.oldest.pop_front() else {
                return;
            };
            if let Some(old) = self.masks.remove(&old) {
                self.bytes -= old.buffer.size();
            }
        }
        self.bytes += mask.buffer.size();
        self.oldest.push_back(key);
        self.masks.insert(key, mask);
    }
}

impl Compositor {
    pub(super) fn gpu_credit_roll(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        media: &MediaLayer<'_>,
        owner: u64,
        ask: Ask,
    ) -> Option<Img> {
        let MediaSource::Generated(Generator::Plugin(fx)) = &media.info.source else {
            return None;
        };
        if fx.id != "org.bananav.CreditRoll" {
            return None;
        }
        let video = media.info.video?;
        let full = (video.width.max(1), video.height.max(1));
        let size = bv_media::plugins::covering(full, ask.min_size);
        let mut roll = self
            .credits
            .remove(&owner)
            .filter(|roll| roll.id == media.info.id && roll.size == size)
            .unwrap_or_else(|| {
                let plan = (|| {
                    if fx.params.values().any(|param| !param.keys.is_empty()) {
                        return None;
                    }
                    let values: Vec<_> =
                        bv_media::plugins::generator_values(fx, video.frame_rate, ask.time)?
                            .into_iter()
                            .filter_map(|(name, value)| match value {
                                PluginValue::Num(value) => Some((name, Value::Num(value))),
                                PluginValue::Text(value) => Some((name, Value::Text(value))),
                                _ => None,
                            })
                            .collect();
                    CreditRoll::new(Values(&values), full, size)
                })();
                Roll {
                    id: media.info.id,
                    size,
                    plan,
                    masks: HashMap::new(),
                    oldest: VecDeque::new(),
                    bytes: 0,
                }
            });
        let start = self.readback_timing.as_ref().map(|_| Instant::now());
        let picture = (|| {
            let plan = roll.plan.as_ref()?;
            let time = video.frame_rate.frame_at(ask.time) as f64;
            let duration = bv_media::plugins::frames(
                ask.span.unwrap_or(Generator::DURATION),
                video.frame_rate,
            );
            let lines = plan.frame(time, duration, video.frame_rate.as_f64())?;
            let out = self.image(size.0, size.1);
            self.kernel(
                enc,
                K::Fill,
                Params::sized(size.0, size.1).with_f(&plan.background()),
                &[&out.buf],
                Grid::Pixels(size.0, size.1),
            );
            for line in lines {
                let mask = match roll.masks.get(&line.key) {
                    Some(mask) => mask.clone(),
                    None => {
                        let mask = self.upload_credit_mask(&line)?;
                        roll.remember(line.key, mask.clone());
                        mask
                    }
                };
                self.place_credit_mask(enc, &out, &mask, &line);
            }
            Some(out)
        })();
        if let (Some(start), Some(timing)) = (start, &mut self.readback_timing)
            && picture.is_some()
        {
            timing.gpu_credit_prepare += start.elapsed();
            timing.gpu_credit_frames += 1;
        }
        self.seen.push(owner);
        self.credits.insert(owner, roll);
        picture
    }

    fn upload_credit_mask(&mut self, line: &CreditLinePlan) -> Option<Mask> {
        let CreditMask {
            origin,
            size,
            mut alpha,
            color,
        } = line.rasterize()?;
        alpha.resize(alpha.len().next_multiple_of(4), 0.0);
        let data: &[u8] = bytemuck::cast_slice(&alpha);
        if data.len() as u64 > CACHE_BYTES {
            return None;
        }
        let start = self.readback_timing.as_ref().map(|_| Instant::now());
        let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("credit line mask"),
            size: data.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.gpu.queue.write_buffer(&buffer, 0, data);
        if let (Some(start), Some(timing)) = (start, &mut self.readback_timing) {
            timing.credit_rolls.upload += start.elapsed();
            timing.credit_rolls.upload_bytes += data.len() as u64;
            timing.credit_rolls.uploads += 1;
        }
        Some(Mask {
            buffer,
            origin,
            size,
            color,
        })
    }

    fn place_credit_mask(
        &self,
        enc: &mut wgpu::CommandEncoder,
        frame: &Img,
        mask: &Mask,
        line: &CreditLinePlan,
    ) {
        let [x, y] = [mask.origin[0], mask.origin[1] + line.y_offset];
        let [left, top] = [x.max(0), y.max(0)];
        let right = (x + i64::from(mask.size[0])).min(i64::from(frame.w));
        let bottom = (y + i64::from(mask.size[1])).min(i64::from(frame.h));
        if left >= right || top >= bottom {
            return;
        }
        let [r, g, b] = mask.color;
        let mut p = Params::sized(frame.w, frame.h).with_f(&[r, g, b, line.opacity]);
        p.sw = mask.size[0];
        p.i = [left as i32, top as i32, x as i32, y as i32];
        p.j = [right as u32, bottom as u32, 0, 0];
        self.kernel(
            enc,
            K::CreditMask,
            p,
            &[&mask.buffer, &frame.buf],
            Grid::Pixels((right - left) as u32, (bottom - top) as u32),
        );
    }
}
