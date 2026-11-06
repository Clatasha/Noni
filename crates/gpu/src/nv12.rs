//! The Windows export bridge: copy the GPU's packed NV12 into a shareable
//! texture. D3D11 and D3D12 can share NV12 on drivers with extended support
//! (Microsoft Learn, "Direct3D 11.4 Features"). The encoder checks the
//! resource on its device before the export starts.
#![allow(unsafe_code)]

use std::mem::ManuallyDrop;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};

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
            })
        }
    }

    fn create(&mut self, dx12: &ID3D12Device, w: u32, h: u32) -> Result<SharedNv12, GpuError> {
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
            Ok(SharedNv12 {
                handle,
                resource,
                _texture: texture,
                planes,
                ready: self.ready_on_dx12.clone(),
                value: self.value,
            })
        }
    }
}

/// One completed frame. Its resource is kept alive until the writer has
/// opened the handle and recorded its copy. Frames are never overwritten.
#[derive(Debug)]
pub struct SharedNv12 {
    handle: OwnedHandle,
    resource: ID3D12Resource,
    _texture: ID3D11Texture2D,
    ready: ID3D12Fence,
    value: u64,
    planes: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 2],
}

impl SharedNv12 {
    pub(crate) fn pitch(&self) -> u32 {
        self.planes[0].Footprint.RowPitch
    }

    pub fn handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
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

/// A private command list for one compositor. The source buffer decays
/// to COMMON after the shader submission and promotes for the copy.
/// The shared texture is explicitly returned to COMMON for D3D11.
pub(crate) struct CopyNv12 {
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    command: ID3D12CommandList,
    queue: ID3D12CommandQueue,
}

fn transition(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    // SAFETY: the command records this borrowed pointer while the frame
    // owns its resource, which stays alive through the submission wait.
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
        // SAFETY: an independent command list on wgpu's D3D12 device and
        // direct queue. No wgpu-owned list or allocator is modified.
        unsafe {
            let hal = gpu.device.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
            let device = hal.raw_device();
            let allocator: ID3D12CommandAllocator = device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(error)?;
            let list: ID3D12GraphicsCommandList = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
                .map_err(error)?;
            list.Close().map_err(error)?;
            let command = list.cast().map_err(error)?;
            let queue = gpu.queue.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
            Ok(Self {
                allocator,
                list,
                command,
                queue: queue.as_raw().clone(),
            })
        }
    }

    pub(crate) fn copy(
        &mut self,
        gpu: &Gpu,
        words: &wgpu::Buffer,
        dst: &SharedNv12,
    ) -> Result<(), GpuError> {
        // SAFETY: the shader was already submitted to the same direct
        // queue. Both resources outlive the completion wait below. The
        // previous use of this list and allocator was also waited on.
        unsafe {
            let hal = words.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
            let source = hal.raw_resource();
            self.allocator.Reset().map_err(error)?;
            self.list.Reset(&self.allocator, None).map_err(error)?;
            self.list.ResourceBarrier(&[transition(
                &dst.resource,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            )]);
            for (i, plane) in dst.planes.iter().enumerate() {
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
                        dst.resource.as_raw(),
                    ))),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: i as u32,
                    },
                };
                self.list.CopyTextureRegion(&target, 0, 0, 0, &src, None);
            }
            self.list.ResourceBarrier(&[transition(
                &dst.resource,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_COMMON,
            )]);
            self.list.Close().map_err(error)?;
            self.queue
                .Wait(&dst.ready, dst.value)
                .map_err(stage("waiting for D3D11 NV12 initialization"))?;
            self.queue
                .ExecuteCommandLists(&[Some(self.command.clone())]);
        }
        // A wgpu queue marker after the native list gives its completion
        // index. D3D11 may only open/use the texture after that wait.
        let submitted = gpu.queue.submit([]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submitted),
                timeout: None,
            })
            .map_err(|e| GpuError::Failed(e.to_string()))?;
        if gpu.is_lost() {
            return Err(GpuError::Lost);
        }
        Ok(())
    }
}
