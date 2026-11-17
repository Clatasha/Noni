//! Share decoder pictures with the compositor without mapping them to the CPU.
use std::os::windows::io::{AsHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;

use bv_core::{ColorInfo, Tick};
use windows::Win32::Foundation::GENERIC_ALL;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::{
    DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIDevice, IDXGIResource1,
};
use windows::Win32::Media::MediaFoundation::{IMFDXGIBuffer, IMFSample};
use windows::core::{Interface, PCWSTR};

pub struct DecodeSurface {
    pub pts: Tick,
    pub width: u32,
    pub height: u32,
    pub offset: (u32, u32),
    pub color: ColorInfo,
    pub adapter: (u32, i32),
    storage: Arc<Storage>,
    fence: Arc<OwnedHandle>,
    value: u64,
}

impl std::fmt::Debug for DecodeSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodeSurface")
            .field("pts", &self.pts)
            .finish_non_exhaustive()
    }
}

impl DecodeSurface {
    pub fn texture_handle(&self) -> BorrowedHandle<'_> {
        self.storage.handle.as_handle()
    }

    pub fn ready(&self) -> (BorrowedHandle<'_>, u64) {
        (self.fence.as_handle(), self.value)
    }
}

struct Storage {
    texture: ID3D11Texture2D,
    handle: OwnedHandle,
    size: (u32, u32),
}

pub(crate) struct SurfacePool {
    device: ID3D11Device,
    context: ID3D11DeviceContext4,
    fence: ID3D11Fence,
    handle: Arc<OwnedHandle>,
    adapter: (u32, i32),
    value: u64,
    storage: Vec<Arc<Storage>>,
}

impl SurfacePool {
    pub(crate) fn copy(
        pool: &mut Option<Self>,
        sample: &IMFSample,
        pts: Tick,
        size: (u32, u32),
        offset: (u32, u32),
        color: ColorInfo,
    ) -> Option<DecodeSurface> {
        // SAFETY: the decoder owns this sample. Its D3D11 device has
        // multithread protection. Reuse requires the renderer to release
        // every reference to the allocation after its GPU copy finishes.
        unsafe {
            let buffer = sample
                .GetBufferByIndex(0)
                .ok()?
                .cast::<IMFDXGIBuffer>()
                .ok()?;
            let mut raw = std::ptr::null_mut();
            buffer.GetResource(&ID3D11Texture2D::IID, &mut raw).ok()?;
            if raw.is_null() {
                return None;
            }
            let source = ID3D11Texture2D::from_raw(raw);
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            if desc.Format != DXGI_FORMAT_NV12
                || size.0.checked_add(offset.0)? > desc.Width
                || size.1.checked_add(offset.1)? > desc.Height
            {
                return None;
            }
            let index = buffer.GetSubresourceIndex().ok()?;
            let device = source.GetDevice().ok()?;
            if pool.as_ref().is_none_or(|p| p.device != device) {
                let adapter = device
                    .cast::<IDXGIDevice>()
                    .ok()?
                    .GetAdapter()
                    .ok()?
                    .GetDesc()
                    .ok()?
                    .AdapterLuid;
                let context = device
                    .GetImmediateContext()
                    .ok()?
                    .cast::<ID3D11DeviceContext4>()
                    .ok()?;
                let mut fence = None;
                device
                    .cast::<ID3D11Device5>()
                    .ok()?
                    .CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut fence)
                    .ok()?;
                let fence: ID3D11Fence = fence?;
                let handle = fence
                    .CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())
                    .ok()?;
                *pool = Some(Self {
                    device,
                    context,
                    fence,
                    handle: Arc::new(OwnedHandle::from_raw_handle(handle.0)),
                    adapter: (adapter.LowPart, adapter.HighPart),
                    value: 0,
                    storage: Vec::new(),
                });
            }
            let pool = pool.as_mut()?;
            let allocation = (desc.Width, desc.Height);
            pool.storage
                .retain(|s| s.size == allocation || Arc::strong_count(s) > 1);
            let storage = if let Some(s) = pool
                .storage
                .iter()
                .find(|s| s.size == allocation && Arc::strong_count(s) == 1)
            {
                s.clone()
            } else {
                // Bound retained allocations per reader. An unusually deep
                // queue can still use the ordinary CPU path.
                if pool.storage.len() >= 16 {
                    return None;
                }
                desc.MipLevels = 1;
                desc.ArraySize = 1;
                desc.Usage = D3D11_USAGE_DEFAULT;
                desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
                desc.CPUAccessFlags = 0;
                desc.MiscFlags =
                    (D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED).0 as u32;
                let mut texture = None;
                pool.device
                    .CreateTexture2D(&desc, None, Some(&mut texture))
                    .ok()?;
                let texture = texture?;
                let handle = texture
                    .cast::<IDXGIResource1>()
                    .ok()?
                    .CreateSharedHandle(
                        None,
                        (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0,
                        PCWSTR::null(),
                    )
                    .ok()?;
                let s = Arc::new(Storage {
                    texture,
                    handle: OwnedHandle::from_raw_handle(handle.0),
                    size: allocation,
                });
                pool.storage.push(s.clone());
                s
            };
            let value = pool.value.checked_add(1)?;
            pool.context
                .CopySubresourceRegion(&storage.texture, 0, 0, 0, 0, &source, index, None);
            pool.context.Signal(&pool.fence, value).ok()?;
            pool.context.Flush();
            pool.value = value;
            Some(DecodeSurface {
                pts,
                width: size.0,
                height: size.1,
                offset,
                color,
                adapter: pool.adapter,
                storage,
                fence: pool.handle.clone(),
                value,
            })
        }
    }
}
