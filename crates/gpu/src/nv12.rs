//! The Windows export bridge: copy the GPU's packed NV12 into a shareable
//! texture. D3D11 and D3D12 can share NV12 on drivers with extended support
//! (Microsoft Learn, "Direct3D 11.4 Features"). The encoder checks the
//! resource on its device before the export starts.
#![allow(unsafe_code)]

use std::mem::ManuallyDrop;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::time::Duration;

use wgpu::hal::api::Dx12;
use windows::Win32::Foundation::{GENERIC_ALL, HANDLE, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIAdapter,
    IDXGIFactory4, IDXGIResource1,
};
use windows::core::{Interface, PCWSTR};

use crate::recycle::Surface;
use crate::{Gpu, GpuError};

fn stage(step: &'static str) -> impl FnOnce(windows::core::Error) -> GpuError {
    move |e| GpuError::Failed(format!("{step}: {e} (0x{:08X})", e.code().0 as u32))
}

fn error(e: windows::core::Error) -> GpuError {
    stage("recording the D3D12 NV12 copy")(e)
}

/// Creates the shareable NV12 surfaces in D3D11 so Media Foundation
/// opens a D3D11-originated resource. D3D12 imports the same allocation.
/// This follows the sharing direction in Dawn's Windows VideoViewsTests.
pub(crate) struct Nv12Device {
    device: ID3D11Device,
    context: ID3D11DeviceContext4,
    ready: ID3D11Fence,
    ready_on_dx12: ID3D12Fence,
    value: u64,
    surfaces: Vec<Arc<Nv12Surface>>,
}

impl Nv12Device {
    fn new(dx12: &ID3D12Device) -> Result<Self, GpuError> {
        // SAFETY: the D3D11 device and its fence are made on the same
        // adapter as D3D12. The temporary handle is closed after import.
        unsafe {
            let factory: IDXGIFactory4 =
                CreateDXGIFactory1().map_err(stage("creating DXGI factory"))?;
            let adapter: IDXGIAdapter = factory
                .EnumAdapterByLuid(dx12.GetAdapterLuid())
                .map_err(stage("locating compositor adapter"))?;
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(stage("creating D3D11 sharing device"))?;
            let device = device.ok_or(GpuError::NoAdapter)?;
            let context = context
                .ok_or(GpuError::NoAdapter)?
                .cast::<ID3D11DeviceContext4>()
                .map_err(stage("opening D3D11 fence context"))?;
            let device5: ID3D11Device5 =
                device.cast().map_err(stage("opening D3D11 fence device"))?;
            let mut ready = None;
            device5
                .CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut ready)
                .map_err(stage("creating D3D11 initialization fence"))?;
            let ready: ID3D11Fence = ready.ok_or(GpuError::NoAdapter)?;
            let handle = ready
                .CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(stage("sharing D3D11 initialization fence"))?;
            let handle = OwnedHandle::from_raw_handle(handle.0);
            let mut ready_on_dx12 = None;
            dx12.OpenSharedHandle(HANDLE(handle.as_raw_handle()), &mut ready_on_dx12)
                .map_err(stage("importing initialization fence into D3D12"))?;
            Ok(Self {
                device,
                context,
                ready,
                ready_on_dx12: ready_on_dx12.ok_or(GpuError::NoAdapter)?,
                value: 0,
                surfaces: Vec::new(),
            })
        }
    }

    fn create(&mut self, dx12: &ID3D12Device, w: u32, h: u32) -> Result<SharedNv12, GpuError> {
        static POOL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let pool =
            *POOL.get_or_init(|| std::env::var("NONI_NV12_SURFACE_POOL").as_deref() != Ok("0"));
        // Only NVENC's completion path arms recycling. MFT frames and probes
        // remain single-use. Both the native-copy callback and encoder slot
        // retain an Arc, so a sole pool reference proves every GPU user ended.
        if pool {
            self.surfaces.retain(Surface::retainable);
            if let Some(surface) = self
                .surfaces
                .iter()
                .find(|s| s.width == w && s.height == h && Surface::available(s))
            {
                return Ok(SharedNv12 {
                    surface: Surface::checkout(surface),
                    completion: None,
                    reused: true,
                });
            }
        }
        // SAFETY: this factory is locked by the caller so its D3D11
        // context and monotonically increasing fence are used in order.
        unsafe {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED).0
                    as u32,
            };
            let mut texture = None;
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .map_err(stage("allocating shared D3D11 NV12 texture"))?;
            let texture = texture.ok_or(GpuError::NoAdapter)?;
            let shared: IDXGIResource1 = texture
                .cast()
                .map_err(stage("querying shareable NV12 texture"))?;
            let handle = shared
                .CreateSharedHandle(
                    None,
                    (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0,
                    PCWSTR::null(),
                )
                .map_err(stage("sharing D3D11 NV12 texture"))?;
            let handle = OwnedHandle::from_raw_handle(handle.0);
            let mut resource = None;
            dx12.OpenSharedHandle(HANDLE(handle.as_raw_handle()), &mut resource)
                .map_err(stage("importing D3D11 NV12 texture into D3D12"))?;
            let resource: ID3D12Resource = resource.ok_or(GpuError::NoAdapter)?;
            let desc = resource.GetDesc();
            let mut planes = [D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); 2];
            dx12.GetCopyableFootprints(&desc, 0, 2, 0, Some(planes.as_mut_ptr()), None, None, None);
            let [y, uv] = planes;
            let pitch = y.Footprint.RowPitch;
            if pitch < w
                || y.Offset != 0
                || uv.Offset != u64::from(pitch) * u64::from(h)
                || uv.Footprint.RowPitch != pitch
            {
                return Err(GpuError::Failed("unsupported NV12 plane layout".into()));
            }
            self.value += 1;
            self.context
                .Signal(&self.ready, self.value)
                .map_err(stage("signaling D3D11 texture initialization"))?;
            self.context.Flush();
            let surface = Arc::new(Surface::new(Nv12SurfaceInner {
                handle,
                resource,
                _texture: texture,
                planes,
                ready: self.ready_on_dx12.clone(),
                value: self.value,
                width: w,
                height: h,
            }));
            if self.surfaces.len() == 24
                && let Some(i) = self.surfaces.iter().position(|s| Arc::strong_count(s) == 1)
            {
                self.surfaces.swap_remove(i);
            }
            // A cap across sizes and painters bounds retained GPU allocations.
            // If all are busy, a single-use allocation preserves progress.
            if pool && self.surfaces.len() < 24 {
                self.surfaces.push(surface.clone());
            }
            Ok(SharedNv12 {
                surface,
                completion: None,
                reused: false,
            })
        }
    }
}

/// One submitted frame. The writer waits for its completion before using
/// the texture, either on the CPU or through a shared GPU fence.
/// Frames are never overwritten.
#[derive(Debug)]
pub struct SharedNv12 {
    surface: Arc<Nv12Surface>,
    completion: Option<Completion>,
    reused: bool,
}

type Nv12Surface = Surface<Nv12SurfaceInner>;

#[derive(Debug)]
struct Nv12SurfaceInner {
    handle: OwnedHandle,
    resource: ID3D12Resource,
    _texture: ID3D11Texture2D,
    ready: ID3D12Fence,
    value: u64,
    planes: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 2],
    width: u32,
    height: u32,
}

impl SharedNv12 {
    pub(crate) fn pitch(&self) -> u32 {
        self.surface.planes[0].Footprint.RowPitch
    }

    pub fn handle(&self) -> BorrowedHandle<'_> {
        self.surface.handle.as_handle()
    }

    /// Retain this source until the consuming encoder's GPU completion event.
    /// Invoke the callback only after successful completion; dropping it after
    /// an error frees the lease without making this allocation reusable.
    /// The native copy's completion callback retains a separate lease. Both
    /// must finish before the pool can hand the allocation out again.
    pub fn encoder_lease(&self) -> Box<dyn FnOnce() + Send> {
        Surface::completion(&self.surface)
    }

    pub fn reused_surface(&self) -> bool {
        self.reused
    }

    /// Fence and value for a D3D11 GPU-side wait. None for an empty probe.
    pub fn completion(&self) -> Option<(BorrowedHandle<'_>, u64)> {
        self.completion
            .as_ref()
            .map(|c| (c.fence.handle.as_handle(), c.value))
    }

    /// CPU fallback for an encoder that cannot wait on a shared fence.
    pub fn wait(&self) -> Result<(), GpuError> {
        if let Some(c) = &self.completion {
            c.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(c.submitted.clone()),
                    timeout: Some(Duration::from_secs(30)),
                })
                .map_err(|e| GpuError::Failed(e.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct CopyFence {
    fence: ID3D12Fence,
    handle: OwnedHandle,
}

#[derive(Debug)]
struct Completion {
    fence: Arc<CopyFence>,
    value: u64,
    device: wgpu::Device,
    submitted: wgpu::SubmissionIndex,
}

impl Gpu {
    /// The DXGI adapter LUID, used to create the encoder on this same card.
    pub fn dx12_luid(&self) -> Option<(u32, i32)> {
        // SAFETY: only querying the adapter of the live wgpu device.
        let hal = unsafe { self.device.as_hal::<Dx12>() }?;
        let luid = unsafe { hal.raw_device().GetAdapterLuid() };
        Some((luid.LowPart, luid.HighPart))
    }

    pub fn shared_nv12(&self, (w, h): (u32, u32)) -> Result<SharedNv12, GpuError> {
        if w == 0 || h == 0 || !w.is_multiple_of(2) || !h.is_multiple_of(2) {
            return Err(GpuError::Failed(
                "NV12 requires even frame dimensions".into(),
            ));
        }
        // SAFETY: the HAL device remains borrowed through the import.
        let hal = unsafe { self.device.as_hal::<Dx12>() }.ok_or(GpuError::NoAdapter)?;
        let dx12 = hal.raw_device();
        let mut factory = self
            .nv12_device
            .lock()
            .map_err(|_| GpuError::Failed("D3D11 sharing device lock failed".into()))?;
        if factory.is_none() {
            *factory = Some(Nv12Device::new(dx12)?);
        }
        factory
            .as_mut()
            .ok_or(GpuError::NoAdapter)?
            .create(dx12, w, h)
    }
}

/// A small ring of native copy command lists. A slot is reset only after
/// its previous submission completes. The source buffer decays to COMMON
/// after the shader submission, and the texture is returned to COMMON.
pub(crate) struct CopyNv12 {
    slots: Vec<CopySlot>,
    next: usize,
    fence: Arc<CopyFence>,
    value: u64,
    queue: ID3D12CommandQueue,
    device: wgpu::Device,
    last: Option<wgpu::SubmissionIndex>,
}

struct CopySlot {
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    command: ID3D12CommandList,
    submitted: Option<wgpu::SubmissionIndex>,
}

// Kept by the queue's completion callback even if export is cancelled or
// the compositor is dropped. D3D12 does not retain command-list resources.
struct CopyLease {
    _allocator: ID3D12CommandAllocator,
    _list: ID3D12GraphicsCommandList,
    _source: wgpu::Buffer,
    _destination: Arc<Nv12Surface>,
    _fence: Arc<CopyFence>,
}

impl CopySlot {
    fn new(device: &ID3D12Device) -> Result<Self, GpuError> {
        // SAFETY: these command objects belong exclusively to one compositor.
        unsafe {
            let allocator: ID3D12CommandAllocator = device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(error)?;
            let list: ID3D12GraphicsCommandList = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
                .map_err(error)?;
            list.Close().map_err(error)?;
            let command = list.cast().map_err(error)?;
            Ok(Self {
                allocator,
                list,
                command,
                submitted: None,
            })
        }
    }
}

fn transition(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    // SAFETY: the command records this borrowed pointer while the frame
    // owns its resource, retained by the lease until submission completes.
    let resource = unsafe { ID3D12Resource::from_raw(resource.as_raw()) };
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(resource)),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

impl CopyNv12 {
    pub(crate) fn new(gpu: &Gpu) -> Result<Self, GpuError> {
        // SAFETY: the fence belongs to wgpu's D3D12 device and is shared
        // with D3D11 through an owned, unnamed handle.
        unsafe {
            let hal = gpu.device.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
            let device = hal.raw_device();
            let fence: ID3D12Fence = device
                .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(stage("creating NV12 completion fence"))?;
            let handle = device
                .CreateSharedHandle(&fence, None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(stage("sharing NV12 completion fence"))?;
            let handle = OwnedHandle::from_raw_handle(handle.0);
            let queue = gpu.queue.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
            Ok(Self {
                slots: Vec::new(),
                next: 0,
                fence: Arc::new(CopyFence { fence, handle }),
                value: 0,
                queue: queue.as_raw().clone(),
                device: gpu.device.clone(),
                last: None,
            })
        }
    }

    pub(crate) fn copy(
        &mut self,
        gpu: &Gpu,
        words: &wgpu::Buffer,
        dst: &mut SharedNv12,
    ) -> Result<(), GpuError> {
        if self.slots.len() < 3 {
            // SAFETY: creating a private command list on the live device.
            let slot = unsafe {
                let hal = gpu.device.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
                CopySlot::new(hal.raw_device())?
            };
            self.slots.push(slot);
        }
        let slot = &mut self.slots[self.next];
        if let Some(submitted) = slot.submitted.take() {
            // Backpressure only when reusing an in-flight command allocator.
            gpu.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submitted),
                    timeout: Some(Duration::from_secs(30)),
                })
                .map_err(|e| GpuError::Failed(e.to_string()))?;
        }
        self.value = self
            .value
            .checked_add(1)
            .ok_or_else(|| GpuError::Failed("NV12 fence counter exhausted".into()))?;
        // SAFETY: shader and copy use the same direct queue. The slot's
        // previous work is complete, and the completion callback below
        // retains the command objects and both resources until this copy ends.
        unsafe {
            let hal = words.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
            let source = hal.raw_resource();
            slot.allocator.Reset().map_err(error)?;
            slot.list.Reset(&slot.allocator, None).map_err(error)?;
            slot.list.ResourceBarrier(&[transition(
                &dst.surface.resource,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            )]);
            for (i, plane) in dst.surface.planes.iter().enumerate() {
                // The copy locations borrow the resources. ManuallyDrop
                // prevents the temporary COM wrappers from releasing them.
                let src = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: ManuallyDrop::new(Some(ID3D12Resource::from_raw(source.as_raw()))),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: *plane,
                    },
                };
                let target = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: ManuallyDrop::new(Some(ID3D12Resource::from_raw(
                        dst.surface.resource.as_raw(),
                    ))),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: i as u32,
                    },
                };
                slot.list.CopyTextureRegion(&target, 0, 0, 0, &src, None);
            }
            slot.list.ResourceBarrier(&[transition(
                &dst.surface.resource,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_COMMON,
            )]);
            slot.list.Close().map_err(error)?;
            self.queue
                .Wait(&dst.surface.ready, dst.surface.value)
                .map_err(stage("waiting for D3D11 NV12 initialization"))?;
            self.queue
                .ExecuteCommandLists(&[Some(slot.command.clone())]);
        }
        // Signal the shared fence after the copy. D3D11 waits for this
        // value before touching the texture. Always retain native objects
        // through a wgpu marker, including if signaling the fence fails.
        let signaled = unsafe { self.queue.Signal(&self.fence.fence, self.value) };
        let submitted = gpu.queue.submit([]);
        let lease = CopyLease {
            _allocator: slot.allocator.clone(),
            _list: slot.list.clone(),
            _source: words.clone(),
            _destination: dst.surface.clone(),
            _fence: self.fence.clone(),
        };
        gpu.queue.on_submitted_work_done(move || drop(lease));
        slot.submitted = Some(submitted.clone());
        self.last = Some(submitted.clone());
        self.next = (self.next + 1) % 3;
        signaled.map_err(stage("signaling NV12 completion"))?;
        dst.completion = Some(Completion {
            fence: self.fence.clone(),
            value: self.value,
            device: gpu.device.clone(),
            submitted,
        });
        if gpu.is_lost() {
            return Err(GpuError::Lost);
        }
        Ok(())
    }
}

impl Drop for CopyNv12 {
    fn drop(&mut self) {
        // Retire the final callbacks when a compositor closes. On a device
        // error, the queue still owns the leases for any outstanding work.
        if let Some(submitted) = self.last.take() {
            let _ = self.device.poll(wgpu::PollType::Wait {
                submission_index: Some(submitted),
                timeout: Some(Duration::from_secs(30)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Compositor, Output, Rendered};
    use bv_core::{ImageRgbaF32, Tick};
    use bv_render::VideoPool;
    use bv_timeline::{Project, ProjectSettings};

    #[test]
    #[ignore = "requires Windows D3D12 with shared NV12 support"]
    fn only_successfully_completed_encoder_leases_allow_reuse() {
        let gpu = Gpu::open(None).unwrap();
        let first = gpu.shared_nv12((64, 32)).unwrap();
        let original = Arc::downgrade(&first.surface);
        let completed = first.encoder_lease();
        drop(first);
        let busy = gpu.shared_nv12((64, 32)).unwrap();
        assert!(!Arc::ptr_eq(&busy.surface, &original.upgrade().unwrap()));
        // Successful encoder completion releases its reference and arms reuse.
        completed();
        let reused = gpu.shared_nv12((64, 32)).unwrap();
        assert!(reused.reused_surface());
        assert!(Arc::ptr_eq(&reused.surface, &original.upgrade().unwrap()));
        let abandoned = reused.encoder_lease();
        drop(reused);
        // A submit failure or cancellation drops the callback without invoking
        // it. An allocation still read by the GPU can never be handed out again.
        drop(abandoned);
        let next = gpu.shared_nv12((64, 32)).unwrap();
        assert!(!next.reused_surface());
        assert!(original.upgrade().is_none());
        drop(busy);
    }

    fn read_frame(gpu: &Gpu, frame: &SharedNv12) -> Vec<u8> {
        let factory = gpu.nv12_device.lock().unwrap();
        let factory = factory.as_ref().unwrap();
        let device: ID3D11Device5 = factory.device.cast().unwrap();
        let (handle, value) = frame.completion().unwrap();
        // SAFETY: the original texture and completion handle remain live.
        // The D3D11 copy waits on the compositor's fence before reading it.
        unsafe {
            let mut fence: Option<ID3D11Fence> = None;
            device
                .OpenSharedFence(HANDLE(handle.as_raw_handle()), &mut fence)
                .unwrap();
            factory.context.Wait(&fence.unwrap(), value).unwrap();
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            frame.surface._texture.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            factory
                .device
                .CreateTexture2D(&desc, None, Some(&mut staging))
                .unwrap();
            let staging = staging.unwrap();
            factory
                .context
                .CopyResource(&staging, &frame.surface._texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            factory
                .context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
            let mut bytes = Vec::new();
            for row in 0..(desc.Height + desc.Height / 2) as usize {
                let source = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
                bytes.extend_from_slice(std::slice::from_raw_parts(source, desc.Width as usize));
            }
            factory.context.Unmap(&staging, 0);
            bytes
        }
    }

    #[test]
    #[ignore = "requires Windows D3D12 with shared NV12 and fence support"]
    fn queued_nv12_frames_survive_command_reuse_and_compositor_drop() {
        let gpu = Gpu::open(None).unwrap();
        assert_eq!(gpu.adapter().backend, "Dx12");
        let mut compositor = Compositor::new(gpu.clone());
        let project = Project::new(ProjectSettings::default());
        let scene = bv_timeline::scene::frame_scene(&project, Tick::ZERO, 64, 32);
        let mut pool = VideoPool::new();
        let colors = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
        ];
        let frames: Vec<_> = colors
            .iter()
            .map(|&color| {
                let Rendered::SharedNv12(frame) = compositor
                    .render(
                        &scene,
                        project.settings.rotation,
                        &mut pool,
                        color,
                        Output::SharedNv12 {
                            video_levels: false,
                        },
                    )
                    .unwrap()
                else {
                    panic!("expected a shared NV12 frame");
                };
                frame
            })
            .collect();
        // Exercise the CPU fallback, then read the other queued frames
        // through GPU waits after all three command slots have been reused.
        frames[0].wait().unwrap();
        for (frame, color) in frames[..4].iter().zip(&colors) {
            let expected = bv_color::linear_to_420(&ImageRgbaF32::filled(64, 32, *color), false);
            assert!(
                read_frame(&gpu, frame)
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.abs_diff(*b) <= 1)
            );
        }
        drop(compositor);
        for (frame, color) in frames[4..].iter().zip(&colors[4..]) {
            let expected = bv_color::linear_to_420(&ImageRgbaF32::filled(64, 32, *color), false);
            let actual = read_frame(&gpu, frame);
            assert_eq!(actual.len(), expected.len());
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.abs_diff(*b) <= 1)
            );
        }
    }
}
