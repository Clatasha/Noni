//! A Direct3D 11 device on a chosen graphics card, for the hardware
//! encoders that only work with one (AMD's): the encoder gets the device
//! through a DXGI device manager and the frames as textures of it.

use std::os::windows::io::{AsRawHandle, BorrowedHandle};

use windows::Win32::Foundation::{HANDLE, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::Win32::Media::MediaFoundation::*;
use windows::core::Interface;

use crate::imp::win;
use crate::{Error, Result, Vendor};

/// PCI vendor ids.
fn pci_id(v: Vendor) -> Option<u32> {
    match v {
        Vendor::Nvidia => Some(0x10DE),
        Vendor::Amd => Some(0x1002),
        Vendor::Intel => Some(0x8086),
        // "QCOM": how Windows names Adreno's maker.
        Vendor::Qualcomm => Some(0x4D4F_4351),
        _ => None,
    }
}

pub(crate) struct Device {
    pub manager: IMFDXGIDeviceManager,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    nvenc_input: bool,
    /// The CPU-writable texture frames are copied through.
    staging: Option<ID3D11Texture2D>,
    nv12_pool: Option<IMFVideoSampleAllocatorEx>,
}

impl Device {
    /// A device on the selected graphics card, or the first of `vendor`.
    pub(crate) fn new(vendor: Vendor, luid: Option<(u32, i32)>) -> Result<Device> {
        let want = pci_id(vendor).ok_or(Error::Unavailable)?;
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(win)?;
            let mut adapter: Option<IDXGIAdapter1> = None;
            for i in 0.. {
                let Ok(a) = factory.EnumAdapters1(i) else {
                    break;
                };
                let desc = a.GetDesc1().map_err(win)?;
                if desc.VendorId == want
                    && luid.is_none_or(|id| {
                        id == (desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart)
                    })
                {
                    adapter = Some(a);
                    break;
                }
            }
            let adapter = adapter.ok_or_else(|| Error::Windows(format!("no {vendor:?} card")))?;
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(win)?;
            let device: ID3D11Device = device.ok_or(Error::Unavailable)?;
            let context = context.ok_or(Error::Unavailable)?;
            // The encoder uses the device from its own threads.
            if let Ok(mt) = context.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }
            let mut token = 0u32;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(win)?;
            let manager: IMFDXGIDeviceManager = manager.ok_or(Error::Unavailable)?;
            manager.ResetDevice(&device, token).map_err(win)?;
            Ok(Device {
                manager,
                device,
                context,
                nvenc_input: luid.is_some(),
                staging: None,
                nv12_pool: None,
            })
        }
    }

    fn texture(
        &self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        staging: bool,
    ) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: if staging {
                D3D11_USAGE_STAGING
            } else {
                D3D11_USAGE_DEFAULT
            },
            BindFlags: if staging {
                0
            } else if self.nvenc_input {
                D3D11_BIND_VIDEO_ENCODER.0 as u32
            } else {
                D3D11_BIND_RENDER_TARGET.0 as u32
            },
            CPUAccessFlags: if staging {
                D3D11_CPU_ACCESS_WRITE.0 as u32
            } else {
                0
            },
            MiscFlags: 0,
        };
        let mut tex = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut tex))
                .map_err(win)?;
        }
        tex.ok_or(Error::Unavailable)
    }

    #[cfg(target_arch = "x86_64")]
    pub(crate) fn raw_device(&self) -> &ID3D11Device {
        &self.device
    }

    #[cfg(target_arch = "x86_64")]
    pub(crate) fn encoder_nv12(&self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        self.texture(width, height, DXGI_FORMAT_NV12, false)
    }

    pub(crate) fn copy_nv12(&self, destination: &ID3D11Texture2D, source: &ID3D11Texture2D) {
        // SAFETY: both textures are opened on this device and validated
        // against the same NV12 dimensions. Submit the copy before encoding.
        unsafe {
            self.context.CopyResource(destination, source);
            self.context.Flush();
        }
    }

    /// Opens the compositor's completed texture on the encoder's device.
    pub(crate) fn shared_nv12(
        &self,
        handle: BorrowedHandle<'_>,
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D> {
        let device = self.device.cast::<ID3D11Device1>().map_err(win)?;
        // SAFETY: the handle is borrowed from the live shared texture.
        // The caller waits for the compositor before encoding.
        unsafe {
            let texture: ID3D11Texture2D = device
                .OpenSharedResource1(HANDLE(handle.as_raw_handle()))
                .map_err(|e| {
                    Error::Windows(format!(
                        "OpenSharedResource1: {e} (0x{:08X})",
                        e.code().0 as u32
                    ))
                })?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            if (desc.Width, desc.Height, desc.Format) != (width, height, DXGI_FORMAT_NV12)
                || desc.ArraySize != 1
                || desc.MipLevels != 1
                || desc.SampleDesc.Count != 1
            {
                return Err(Error::Windows(
                    "unexpected shared NV12 texture layout".into(),
                ));
            }
            Ok(texture)
        }
    }

    /// A bounded pool of samples on the encoder device. Media Foundation
    /// recycles them when the asynchronous encoder releases its reference.
    /// https://learn.microsoft.com/windows/win32/api/mfidl/nn-mfidl-imfvideosampleallocatorex
    pub(crate) fn enable_nv12_pool(
        &mut self,
        media_type: &IMFMediaType,
        width: u32,
        height: u32,
    ) -> Result<()> {
        // SAFETY: COM is initialized and the media type is the encoder's
        // negotiated NV12 input. The manager owns this device.
        unsafe {
            let mut raw = std::ptr::null_mut();
            MFCreateVideoSampleAllocatorEx(&IMFVideoSampleAllocatorEx::IID, &mut raw)
                .map_err(win)?;
            if raw.is_null() {
                return Err(Error::Unavailable);
            }
            let pool = IMFVideoSampleAllocatorEx::from_raw(raw);
            pool.SetDirectXManager(&self.manager).map_err(win)?;
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 3).map_err(win)?;
            let attrs = attrs.ok_or(Error::Unavailable)?;
            attrs.SetUINT32(&MF_SA_BUFFERS_PER_SAMPLE, 1).map_err(win)?;
            attrs
                .SetUINT32(&MF_SA_D3D11_BINDFLAGS, D3D11_BIND_VIDEO_ENCODER.0 as u32)
                .map_err(win)?;
            attrs
                .SetUINT32(&MF_SA_D3D11_USAGE, D3D11_USAGE_DEFAULT.0 as u32)
                .map_err(win)?;
            pool.InitializeSampleAllocatorEx(4, 16, &attrs, media_type)
                .map_err(win)?;
            self.nv12_pool = Some(pool);
        }
        if self.pooled_nv12(width, height).is_none() {
            self.nv12_pool = None;
            return Err(Error::Unavailable);
        }
        Ok(())
    }

    /// An available sample and its texture. Keep the same standalone
    /// surface layout as the existing NVIDIA input path.
    fn pooled_nv12(&self, width: u32, height: u32) -> Option<(IMFSample, ID3D11Texture2D)> {
        let pool = self.nv12_pool.as_ref()?;
        // SAFETY: the allocator returns a sample reserved for this caller.
        // GetResource gives us an owned COM reference on success.
        unsafe {
            let sample = pool.AllocateSample().ok()?;
            let buffer = sample.GetBufferByIndex(0).ok()?;
            let dxgi = buffer.cast::<IMFDXGIBuffer>().ok()?;
            if dxgi.GetSubresourceIndex().ok()? != 0 {
                return None;
            }
            let mut raw = std::ptr::null_mut();
            dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw).ok()?;
            if raw.is_null() {
                return None;
            }
            let texture = ID3D11Texture2D::from_raw(raw);
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            if (desc.Width, desc.Height, desc.Format) != (width, height, DXGI_FORMAT_NV12)
                || desc.ArraySize != 1
                || desc.MipLevels != 1
                || desc.SampleDesc.Count != 1
                || desc.BindFlags & D3D11_BIND_VIDEO_ENCODER.0 as u32 == 0
            {
                return None;
            }
            let length = buffer
                .cast::<IMF2DBuffer>()
                .ok()?
                .GetContiguousLength()
                .ok()?;
            buffer.SetCurrentLength(length).ok()?;
            Some((sample, texture))
        }
    }

    /// Copy into a private encoder surface. If the pool is unavailable or
    /// full, allocate a one-off sample so the writer can keep progressing.
    pub(crate) fn copy_shared_nv12(
        &self,
        handle: BorrowedHandle<'_>,
        width: u32,
        height: u32,
    ) -> Result<(IMFSample, bool)> {
        let shared = self.shared_nv12(handle, width, height)?;
        let (sample, frame, pooled) = match self.pooled_nv12(width, height) {
            Some((sample, frame)) => (sample, frame, true),
            None => {
                let frame = self.texture(width, height, DXGI_FORMAT_NV12, false)?;
                let buffer = Self::media_buffer(&frame)?;
                (crate::encode::sample_from_buffer(&buffer)?, frame, false)
            }
        };
        self.copy_nv12(&frame, &shared);
        Ok((sample, pooled))
    }

    fn media_buffer(frame: &ID3D11Texture2D) -> Result<IMFMediaBuffer> {
        // SAFETY: Media Foundation keeps its own reference to this texture.
        unsafe {
            let buffer =
                MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, frame, 0, false).map_err(win)?;
            if let Ok(b2) = buffer.cast::<IMF2DBuffer>()
                && let Ok(n) = b2.GetContiguousLength()
            {
                let _ = buffer.SetCurrentLength(n);
            }
            Ok(buffer)
        }
    }

    /// A media buffer holding `data` (NV12 or P010: `height` rows of luma,
    /// then the chroma rows) as a texture of the device.
    pub(crate) fn upload(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        ten_bit: bool,
    ) -> Result<IMFMediaBuffer> {
        let format = if ten_bit {
            DXGI_FORMAT_P010
        } else {
            DXGI_FORMAT_NV12
        };
        // A new texture per frame: the encoder may still be reading the last.
        let frame = self.texture(width, height, format, false)?;
        self.upload_to(data, width, height, format, &frame)?;
        Self::media_buffer(&frame)
    }

    #[cfg(target_arch = "x86_64")]
    pub(crate) fn upload_nv12(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        frame: &ID3D11Texture2D,
    ) -> Result<()> {
        self.upload_to(data, width, height, DXGI_FORMAT_NV12, frame)
    }

    fn upload_to(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        frame: &ID3D11Texture2D,
    ) -> Result<()> {
        let row = width as usize * if format == DXGI_FORMAT_P010 { 2 } else { 1 };
        let rows = height as usize + height as usize / 2;
        if row.checked_mul(rows).is_none_or(|size| data.len() < size) {
            return Err(Error::Windows("incomplete packed encoder frame".into()));
        }
        if self.staging.is_none() {
            self.staging = Some(self.texture(width, height, format, true)?);
        }
        let staging = self.staging.as_ref().ok_or(Error::Unavailable)?;
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(staging, 0, D3D11_MAP_WRITE, 0, Some(&mut mapped))
                .map_err(win)?;
            let pitch = mapped.RowPitch as usize;
            let dst = mapped.pData as *mut u8;
            // Luma rows, then the interleaved chroma rows right after them.
            for r in 0..rows {
                let src = &data[r * row..(r + 1) * row];
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst.add(r * pitch), row);
            }
            self.context.Unmap(staging, 0);
            self.context.CopyResource(frame, staging);
            if self.nvenc_input {
                self.context.Flush();
            }
        }
        Ok(())
    }
}
