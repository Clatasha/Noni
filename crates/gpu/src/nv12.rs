//! The Windows export bridge: copy the GPU's packed NV12 into a shareable
//! texture. D3D11 and D3D12 can share NV12 on drivers with extended support
//! (Microsoft Learn, "Direct3D 11.4 Features"). The encoder checks the
//! resource on its device before the export starts.
#![allow(unsafe_code)]

use std::mem::ManuallyDrop;
use std::os::windows::io::{AsHandle, BorrowedHandle, FromRawHandle, OwnedHandle};

use wgpu::hal::api::Dx12;
use windows::Win32::Foundation::GENERIC_ALL;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::core::{Interface, PCWSTR};

use crate::{Gpu, GpuError};

fn error(e: windows::core::Error) -> GpuError {
    GpuError::Failed(format!("NV12 texture handoff: {e}"))
}

/// One completed frame. Its resource is kept alive until the writer has
/// opened the handle and recorded its copy. Frames are never overwritten.
#[derive(Debug)]
pub struct SharedNv12 {
    handle: OwnedHandle,
    resource: ID3D12Resource,
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
        // SAFETY: this only allocates on the live wgpu device. The new
        // resource is private until its GPU copy has completed.
        unsafe {
            let hal = self.device.as_hal::<Dx12>().ok_or(GpuError::NoAdapter)?;
            let device = hal.raw_device();
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: u64::from(w),
                Height: h,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
                ..Default::default()
            };
            let heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };
            let mut resource = None;
            device
                .CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_SHARED,
                    &desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )
                .map_err(error)?;
            let resource: ID3D12Resource = resource.ok_or(GpuError::Lost)?;
            let handle = device
                .CreateSharedHandle(&resource, None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(error)?;
            let handle = OwnedHandle::from_raw_handle(handle.0);
            let mut planes = [D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); 2];
            device.GetCopyableFootprints(
                &desc,
                0,
                2,
                0,
                Some(planes.as_mut_ptr()),
                None,
                None,
                None,
            );
            let [y, uv] = planes;
            let pitch = y.Footprint.RowPitch;
            if pitch < w
                || y.Offset != 0
                || uv.Offset != u64::from(pitch) * u64::from(h)
                || uv.Footprint.RowPitch != pitch
            {
                return Err(GpuError::Failed("unsupported NV12 plane layout".into()));
            }
            Ok(SharedNv12 {
                handle,
                resource,
                planes,
            })
        }
    }
}

/// A private command list for one compositor. D3D12 buffers and
/// simultaneous-access textures promote from COMMON for copies and decay
/// back to COMMON once this separate ExecuteCommandLists call completes.
pub(crate) struct CopyNv12 {
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    command: ID3D12CommandList,
    queue: ID3D12CommandQueue,
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
            self.list.Close().map_err(error)?;
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
