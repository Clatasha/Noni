//! Experimental H.264 encoding using the NVIDIA driver's D3D11 API.
//! GPU surfaces stay registered for the session and can be reused only
//! after their completion event, bitstream unlock, and unmap.

use std::ffi::c_void;
use std::os::windows::io::BorrowedHandle;
use std::ptr::{self, NonNull};
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::core::{IUnknown, Interface};

use crate::d3d::Device;
use crate::{
    Codec, Error, Packet, RateControl, Result, SharedNv12Timing, Vendor, VideoEncoderSettings,
};

// Matches SLOT_COUNT in nvenc.c. With no B-frames or lookahead, the
// NVIDIA guide recommends at least four input/output slots.
const SLOTS: u32 = 6;

#[repr(C)]
struct Settings {
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    bitrate: u32,
    max_bitrate: u32,
    gop: u32,
    cbr: u32,
}

#[repr(C)]
struct PacketView {
    data: *const u8,
    size: u32,
    keyframe: u32,
    timestamp: u64,
    duration: u64,
}

unsafe extern "C" {
    fn noni_nvenc_open(
        device: *mut c_void,
        settings: *const Settings,
        output: *mut *mut c_void,
    ) -> i32;
    fn noni_nvenc_register(session: *mut c_void, index: u32, texture: *mut c_void) -> i32;
    fn noni_nvenc_submit(session: *mut c_void, index: u32, time: u64, duration: u64) -> i32;
    fn noni_nvenc_receive(
        session: *mut c_void,
        index: u32,
        blocking: i32,
        out: *mut PacketView,
    ) -> i32;
    fn noni_nvenc_release(session: *mut c_void, index: u32) -> i32;
    fn noni_nvenc_end(session: *mut c_void) -> i32;
    fn noni_nvenc_wait_end(session: *mut c_void) -> i32;
    fn noni_nvenc_close(session: *mut c_void);
}

fn check(stage: &str, status: i32) -> Result<()> {
    let reason = match status {
        0 => return Ok(()),
        -1 => "NVIDIA driver DLL unavailable",
        -2 => "driver lacks NVENC API 12.1 or asynchronous encode support",
        -3 => "session allocation failed",
        -4 => "Windows completion event failed",
        -5 => "timed out waiting for NVIDIA encoder",
        -6 => "unexpected encoder or packet state",
        _ => return Err(Error::Windows(format!("{stage}: NVENC status {status}"))),
    };
    Err(Error::Windows(format!("{stage}: {reason}")))
}

struct Session(NonNull<c_void>);

impl Session {
    fn raw(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the session is owned here and the device and textures
        // remain alive until after it is closed.
        unsafe { noni_nvenc_close(self.raw()) };
    }
}

pub struct DirectNvenc {
    // Drop the session before its registered textures and device.
    session: Session,
    textures: Vec<ID3D11Texture2D>,
    device: Device,
    width: u32,
    height: u32,
    oldest: u32,
    pending: u32,
}

impl std::fmt::Debug for DirectNvenc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectNvenc")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

// SAFETY: exclusive access to the session transfers to the writer thread.
// The D3D11 device is multithread-protected and resources stay owned here.
unsafe impl Send for DirectNvenc {}

impl DirectNvenc {
    pub fn new(
        s: VideoEncoderSettings,
        adapter: (u32, i32),
        probe: BorrowedHandle<'_>,
    ) -> Result<Self> {
        if s.vendor != Vendor::Nvidia
            || s.codec != Codec::H264
            || s.ten_bit
            || s.field_order != bv_core::FieldOrder::Progressive
            || matches!(s.rate_control, RateControl::Quality(_))
        {
            return Err(Error::Windows(
                "direct NVENC requires progressive, 8-bit NVIDIA H.264 with VBR or CBR".into(),
            ));
        }
        crate::imp::init()?;
        let device = Device::new(s.vendor, Some(adapter))?;
        device.shared_nv12(probe, s.width, s.height)?;
        device.check_shared_fence_support()?;
        let textures: Vec<_> = (0..SLOTS)
            .map(|_| device.encoder_nv12(s.width, s.height))
            .collect::<Result<_>>()?;
        let settings = Settings {
            width: s.width,
            height: s.height,
            fps_num: s.frame_rate.num,
            fps_den: s.frame_rate.den,
            bitrate: s.bitrate.max(1),
            max_bitrate: s.max_bitrate.max(s.bitrate).max(1),
            gop: s.gop.max(1),
            cbr: u32::from(s.rate_control == RateControl::Cbr),
        };
        let unknown: IUnknown = device.raw_device().cast().map_err(crate::imp::win)?;
        let mut raw = ptr::null_mut();
        // SAFETY: COM interfaces, input settings, and output pointer are live.
        check("opening direct NVENC", unsafe {
            noni_nvenc_open(unknown.as_raw(), &settings, &mut raw)
        })?;
        let session = Session(NonNull::new(raw).ok_or(Error::Unavailable)?);
        for (i, texture) in textures.iter().enumerate() {
            // SAFETY: textures are private to this session and outlive it.
            check("registering NVENC texture", unsafe {
                noni_nvenc_register(session.raw(), i as u32, texture.as_raw())
            })?;
        }
        Ok(Self {
            session,
            textures,
            device,
            width: s.width,
            height: s.height,
            oldest: 0,
            pending: 0,
        })
    }

    pub fn encode_shared_nv12(
        &mut self,
        handle: BorrowedHandle<'_>,
        completion: Option<(BorrowedHandle<'_>, u64)>,
        time: i64,
        duration: i64,
        timing: Option<&mut SharedNv12Timing>,
    ) -> Result<Vec<Packet>> {
        let (width, height) = (self.width, self.height);
        self.encode(time, duration, timing, |device, texture| {
            let source = device.shared_nv12(handle, width, height)?;
            if let Some((fence, value)) = completion {
                device.wait_shared_fence(fence, value)?;
            }
            device.copy_nv12(texture, &source);
            Ok(())
        })
    }

    pub fn encode_packed(&mut self, data: &[u8], time: i64, duration: i64) -> Result<Vec<Packet>> {
        let (width, height) = (self.width, self.height);
        self.encode(time, duration, None, |device, texture| {
            device.upload_nv12(data, width, height, texture)
        })
    }

    fn encode(
        &mut self,
        time: i64,
        duration: i64,
        timing: Option<&mut SharedNv12Timing>,
        prepare_input: impl FnOnce(&mut Device, &ID3D11Texture2D) -> Result<()>,
    ) -> Result<Vec<Packet>> {
        let started = timing.as_ref().map(|_| Instant::now());
        let mut out = Vec::new();
        let mut event_wait = Duration::ZERO;
        // Submit into the ring without a CPU wait while slots remain. When
        // full, recycle the oldest slot only after its GPU work completes.
        if self.pending == SLOTS {
            self.receive(true, &mut out, timing.is_some().then_some(&mut event_wait))?;
        }
        let index = (self.oldest + self.pending) % SLOTS;
        let copy_started = timing.as_ref().map(|_| Instant::now());
        prepare_input(&mut self.device, &self.textures[index as usize])?;
        let prepare = copy_started
            .map(|start| start.elapsed())
            .unwrap_or_default();
        // SAFETY: index points to a free slot. The copy was submitted on
        // the same D3D11 device passed to NVENC.
        check("submitting NVENC frame", unsafe {
            noni_nvenc_submit(self.session.raw(), index, time as u64, duration as u64)
        })?;
        self.pending += 1;
        while self.pending > 0 && self.receive(false, &mut out, None)? {}
        if let (Some(timing), Some(started)) = (timing, started) {
            timing.pooled = true;
            timing.prepare = prepare;
            timing.event_wait = event_wait;
            timing.encode = started.elapsed().saturating_sub(prepare + event_wait);
        }
        Ok(out)
    }

    fn receive(
        &mut self,
        blocking: bool,
        out: &mut Vec<Packet>,
        wait: Option<&mut Duration>,
    ) -> Result<bool> {
        let mut view = PacketView {
            data: ptr::null(),
            size: 0,
            keyframe: 0,
            timestamp: 0,
            duration: 0,
        };
        let started = wait.as_ref().map(|_| Instant::now());
        // SAFETY: the caller ensures there is a pending slot. A successful
        // receive holds its bitstream lock until release.
        check("receiving NVENC packet", unsafe {
            noni_nvenc_receive(
                self.session.raw(),
                self.oldest,
                i32::from(blocking),
                &mut view,
            )
        })?;
        if let (Some(wait), Some(started)) = (wait, started) {
            *wait += started.elapsed();
        }
        if view.data.is_null() {
            return Ok(false);
        }
        // SAFETY: the driver owns a live allocation of view.size bytes and
        // the lock is held until after the copy.
        let data = unsafe { std::slice::from_raw_parts(view.data, view.size as usize) }.to_vec();
        check("releasing NVENC slot", unsafe {
            noni_nvenc_release(self.session.raw(), self.oldest)
        })?;
        self.oldest = (self.oldest + 1) % SLOTS;
        self.pending -= 1;
        out.push(Packet {
            data,
            time: view.timestamp as i64,
            duration: view.duration as i64,
            decode_time: None,
            keyframe: view.keyframe != 0,
        });
        Ok(true)
    }

    pub fn finish(&mut self) -> Result<Vec<Packet>> {
        // SAFETY: this owns the live session and has stopped submitting.
        check("draining NVENC", unsafe {
            noni_nvenc_end(self.session.raw())
        })?;
        let mut out = Vec::new();
        while self.pending > 0 {
            self.receive(true, &mut out, None)?;
        }
        check("finishing NVENC", unsafe {
            noni_nvenc_wait_end(self.session.raw())
        })?;
        Ok(out)
    }
}
