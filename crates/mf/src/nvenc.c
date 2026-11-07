/* NVIDIA driver ABI bridge. The Rust caller owns the D3D11 device and textures.
 * See NVIDIA Video Codec SDK, NVENC Video Encoder API Programming Guide:
 * https://docs.nvidia.com/video-technologies/video-codec-sdk/11.1/pdf/NVENC_VideoEncoder_API_ProgGuide.pdf
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdlib.h>
#include "nvEncodeAPI.h"

enum { SLOT_COUNT = 6, TIMEOUT_MS = 10000 };
enum { DLL_UNAVAILABLE = -1, API_UNAVAILABLE = -2, ALLOCATION_FAILED = -3,
       EVENT_FAILED = -4, EVENT_TIMEOUT = -5, INVALID_STATE = -6 };

typedef struct {
    uint32_t width, height, fps_num, fps_den, bitrate, max_bitrate, gop, cbr;
} EncoderSettings;

typedef struct {
    const uint8_t *data;
    uint32_t size;
    uint32_t keyframe;
    uint64_t timestamp;
    uint64_t duration;
} EncodedPacket;

typedef struct {
    NV_ENC_REGISTERED_PTR texture;
    NV_ENC_INPUT_PTR mapped;
    NV_ENC_OUTPUT_PTR bitstream;
    HANDLE event;
    int registered_event;
    int locked;
} Slot;

typedef struct {
    HMODULE library;
    NV_ENCODE_API_FUNCTION_LIST api;
    void *session;
    Slot slots[SLOT_COUNT];
    HANDLE eos_event;
    int registered_eos;
    int eos_sent;
    int eos_complete;
    int eos_waited;
    int submitted;
    uint32_t width, height;
} Encoder;

static int register_event(Encoder *enc, HANDLE *event, int *registered)
{
    NV_ENC_EVENT_PARAMS params = {0};
    *event = CreateEventW(NULL, FALSE, FALSE, NULL);
    if (!*event)
        return EVENT_FAILED;
    params.version = NV_ENC_EVENT_PARAMS_VER;
    params.completionEvent = *event;
    NVENCSTATUS status = enc->api.nvEncRegisterAsyncEvent(enc->session, &params);
    *registered = status == NV_ENC_SUCCESS;
    return status;
}

static void unregister_event(Encoder *enc, HANDLE event, int registered)
{
    if (registered) {
        NV_ENC_EVENT_PARAMS params = {0};
        params.version = NV_ENC_EVENT_PARAMS_VER;
        params.completionEvent = event;
        enc->api.nvEncUnregisterAsyncEvent(enc->session, &params);
    }
}

static int wait_event(HANDLE event, DWORD timeout)
{
    DWORD result = WaitForSingleObject(event, timeout);
    if (result == WAIT_OBJECT_0)
        return 0;
    return result == WAIT_TIMEOUT ? EVENT_TIMEOUT : EVENT_FAILED;
}

int noni_nvenc_end(Encoder *enc)
{
    NV_ENC_PIC_PARAMS pic = {0};
    if (enc->eos_sent)
        return 0;
    pic.version = NV_ENC_PIC_PARAMS_VER;
    pic.encodePicFlags = NV_ENC_PIC_FLAG_EOS;
    pic.completionEvent = enc->eos_event;
    NVENCSTATUS status = enc->api.nvEncEncodePicture(enc->session, &pic);
    enc->eos_sent = status == NV_ENC_SUCCESS;
    return status;
}

int noni_nvenc_wait_end(Encoder *enc)
{
    if (!enc->eos_sent)
        return INVALID_STATE;
    if (enc->eos_complete)
        return 0;
    enc->eos_waited = 1;
    int status = wait_event(enc->eos_event, TIMEOUT_MS);
    enc->eos_complete = status == 0;
    return status;
}

void noni_nvenc_close(Encoder *enc)
{
    if (enc->session) {
        // On early exit, give in-flight frames a chance to complete before
        // releasing their registered surfaces and event handles.
        if (enc->submitted && !enc->eos_waited && !noni_nvenc_end(enc))
            noni_nvenc_wait_end(enc);
        for (unsigned i = 0; i < SLOT_COUNT; ++i) {
            Slot *slot = &enc->slots[i];
            if (slot->locked)
                enc->api.nvEncUnlockBitstream(enc->session, slot->bitstream);
            if (slot->mapped)
                enc->api.nvEncUnmapInputResource(enc->session, slot->mapped);
            if (slot->texture)
                enc->api.nvEncUnregisterResource(enc->session, slot->texture);
            if (slot->bitstream)
                enc->api.nvEncDestroyBitstreamBuffer(enc->session, slot->bitstream);
            unregister_event(enc, slot->event, slot->registered_event);
        }
        unregister_event(enc, enc->eos_event, enc->registered_eos);
        enc->api.nvEncDestroyEncoder(enc->session);
    }
    for (unsigned i = 0; i < SLOT_COUNT; ++i)
        if (enc->slots[i].event)
            CloseHandle(enc->slots[i].event);
    if (enc->eos_event)
        CloseHandle(enc->eos_event);
    if (enc->library)
        FreeLibrary(enc->library);
    free(enc);
}

static int open_session(Encoder *enc, void *device, const EncoderSettings *s)
{
    // NVIDIA's current display driver installs this DLL in System32.
    enc->library = LoadLibraryExW(L"nvEncodeAPI64.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!enc->library)
        return DLL_UNAVAILABLE;
    typedef NVENCSTATUS (NVENCAPI *GetVersion)(uint32_t *);
    typedef NVENCSTATUS (NVENCAPI *CreateInstance)(NV_ENCODE_API_FUNCTION_LIST *);
    GetVersion get_version = (GetVersion)(void *)GetProcAddress(enc->library, "NvEncodeAPIGetMaxSupportedVersion");
    CreateInstance create = (CreateInstance)(void *)GetProcAddress(enc->library, "NvEncodeAPICreateInstance");
    uint32_t version = 0;
    if (!get_version || !create || get_version(&version) != NV_ENC_SUCCESS ||
        version < ((NVENCAPI_MAJOR_VERSION << 4) | NVENCAPI_MINOR_VERSION))
        return API_UNAVAILABLE;
    enc->api.version = NV_ENCODE_API_FUNCTION_LIST_VER;
    NVENCSTATUS status = create(&enc->api);
    if (status != NV_ENC_SUCCESS)
        return status;

    NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS open = {0};
    open.version = NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER;
    open.apiVersion = NVENCAPI_VERSION;
    open.deviceType = NV_ENC_DEVICE_TYPE_DIRECTX;
    open.device = device;
    void *session = NULL;
    status = enc->api.nvEncOpenEncodeSessionEx(&open, &session);
    if (status != NV_ENC_SUCCESS)
        return status;

    enc->session = session;
    NV_ENC_CAPS_PARAM caps = {0};
    caps.version = NV_ENC_CAPS_PARAM_VER;
    caps.capsToQuery = NV_ENC_CAPS_ASYNC_ENCODE_SUPPORT;
    int supports_async = 0;
    status = enc->api.nvEncGetEncodeCaps(enc->session, NV_ENC_CODEC_H264_GUID, &caps, &supports_async);
    if (status != NV_ENC_SUCCESS || !supports_async)
        return status != NV_ENC_SUCCESS ? (int)status : API_UNAVAILABLE;

    NV_ENC_PRESET_CONFIG preset = {0};
    preset.version = NV_ENC_PRESET_CONFIG_VER;
    preset.presetCfg.version = NV_ENC_CONFIG_VER;
    status = enc->api.nvEncGetEncodePresetConfigEx(enc->session, NV_ENC_CODEC_H264_GUID,
        NV_ENC_PRESET_P4_GUID, NV_ENC_TUNING_INFO_HIGH_QUALITY, &preset);
    if (status != NV_ENC_SUCCESS)
        return status;

    NV_ENC_CONFIG *config = &preset.presetCfg;
    config->profileGUID = NV_ENC_H264_PROFILE_HIGH_GUID;
    config->gopLength = s->gop;
    config->frameIntervalP = 1; // MP4 packets stay in display order.
    config->rcParams.enableLookahead = 0;
    config->rcParams.multiPass = NV_ENC_MULTI_PASS_DISABLED;
    config->rcParams.rateControlMode = s->cbr ? NV_ENC_PARAMS_RC_CBR : NV_ENC_PARAMS_RC_VBR;
    config->rcParams.averageBitRate = s->bitrate;
    config->rcParams.maxBitRate = s->cbr ? s->bitrate : s->max_bitrate;
    NV_ENC_CONFIG_H264 *h264 = &config->encodeCodecConfig.h264Config;
    h264->idrPeriod = s->gop;
    h264->useBFramesAsRef = NV_ENC_BFRAME_REF_MODE_DISABLED;
    h264->disableSPSPPS = 0;
    h264->repeatSPSPPS = 1;
    NV_ENC_CONFIG_H264_VUI_PARAMETERS *vui = &h264->h264VUIParameters;
    vui->videoSignalTypePresentFlag = 1;
    vui->videoFormat = NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED;
    vui->videoFullRangeFlag = 0;
    vui->colourDescriptionPresentFlag = 1;
    vui->colourPrimaries = NV_ENC_VUI_COLOR_PRIMARIES_BT709;
    vui->transferCharacteristics = NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709;
    vui->colourMatrix = NV_ENC_VUI_MATRIX_COEFFS_BT709;

    NV_ENC_INITIALIZE_PARAMS init = {0};
    init.version = NV_ENC_INITIALIZE_PARAMS_VER;
    init.encodeGUID = NV_ENC_CODEC_H264_GUID;
    init.presetGUID = NV_ENC_PRESET_P4_GUID;
    init.tuningInfo = NV_ENC_TUNING_INFO_HIGH_QUALITY;
    init.encodeWidth = init.darWidth = s->width;
    init.encodeHeight = init.darHeight = s->height;
    init.frameRateNum = s->fps_num;
    init.frameRateDen = s->fps_den;
    init.enableEncodeAsync = 1;
    init.enablePTD = 1;
    init.encodeConfig = config;
    status = enc->api.nvEncInitializeEncoder(enc->session, &init);
    if (status != NV_ENC_SUCCESS)
        return status;
    enc->width = s->width;
    enc->height = s->height;
    return register_event(enc, &enc->eos_event, &enc->registered_eos);
}

int noni_nvenc_open(void *device, const EncoderSettings *settings, Encoder **output)
{
    Encoder *enc = calloc(1, sizeof(*enc));
    if (!enc)
        return ALLOCATION_FAILED;
    int status = open_session(enc, device, settings);
    if (status) {
        noni_nvenc_close(enc);
        return status;
    }
    *output = enc;
    return 0;
}

int noni_nvenc_register(Encoder *enc, uint32_t index, void *texture)
{
    if (index >= SLOT_COUNT || enc->slots[index].texture)
        return INVALID_STATE;
    Slot *slot = &enc->slots[index];
    NV_ENC_REGISTER_RESOURCE resource = {0};
    resource.version = NV_ENC_REGISTER_RESOURCE_VER;
    resource.resourceType = NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX;
    resource.resourceToRegister = texture;
    resource.width = enc->width;
    resource.height = enc->height;
    resource.bufferFormat = NV_ENC_BUFFER_FORMAT_NV12;
    resource.bufferUsage = NV_ENC_INPUT_IMAGE;
    NVENCSTATUS status = enc->api.nvEncRegisterResource(enc->session, &resource);
    if (status != NV_ENC_SUCCESS)
        return status;
    slot->texture = resource.registeredResource;

    NV_ENC_CREATE_BITSTREAM_BUFFER buffer = {0};
    buffer.version = NV_ENC_CREATE_BITSTREAM_BUFFER_VER;
    status = enc->api.nvEncCreateBitstreamBuffer(enc->session, &buffer);
    if (status != NV_ENC_SUCCESS)
        return status;
    slot->bitstream = buffer.bitstreamBuffer;
    return register_event(enc, &slot->event, &slot->registered_event);
}

int noni_nvenc_submit(Encoder *enc, uint32_t index, uint64_t timestamp, uint64_t duration)
{
    if (index >= SLOT_COUNT || enc->eos_sent)
        return INVALID_STATE;
    Slot *slot = &enc->slots[index];
    if (slot->mapped || !slot->registered_event)
        return INVALID_STATE;
    NV_ENC_MAP_INPUT_RESOURCE map = {0};
    map.version = NV_ENC_MAP_INPUT_RESOURCE_VER;
    map.registeredResource = slot->texture;
    NVENCSTATUS status = enc->api.nvEncMapInputResource(enc->session, &map);
    if (status != NV_ENC_SUCCESS)
        return status;
    slot->mapped = map.mappedResource;

    NV_ENC_PIC_PARAMS pic = {0};
    pic.version = NV_ENC_PIC_PARAMS_VER;
    pic.inputWidth = enc->width;
    pic.inputHeight = enc->height;
    pic.inputBuffer = slot->mapped;
    pic.bufferFmt = map.mappedBufferFmt;
    pic.pictureStruct = NV_ENC_PIC_STRUCT_FRAME;
    pic.outputBitstream = slot->bitstream;
    pic.completionEvent = slot->event;
    pic.inputTimeStamp = timestamp;
    pic.inputDuration = duration;
    for (unsigned retry = 0; ; ++retry) {
        status = enc->api.nvEncEncodePicture(enc->session, &pic);
        if (status != NV_ENC_ERR_ENCODER_BUSY || retry == 100)
            break;
        Sleep(1);
    }
    if (status != NV_ENC_SUCCESS && status != NV_ENC_ERR_NEED_MORE_INPUT)
        return status;
    enc->submitted = 1;
    return 0;
}

int noni_nvenc_receive(Encoder *enc, uint32_t index, int blocking, EncodedPacket *out)
{
    if (index >= SLOT_COUNT || !enc->slots[index].mapped || enc->slots[index].locked)
        return INVALID_STATE;
    Slot *slot = &enc->slots[index];
    int status = wait_event(slot->event, blocking ? TIMEOUT_MS : 0);
    if (status)
        return !blocking && status == EVENT_TIMEOUT ? 0 : status;

    NV_ENC_LOCK_BITSTREAM lock = {0};
    lock.version = NV_ENC_LOCK_BITSTREAM_VER;
    lock.outputBitstream = slot->bitstream;
    lock.doNotWait = 1;
    status = enc->api.nvEncLockBitstream(enc->session, &lock);
    if (status)
        return status;
    slot->locked = 1;
    out->data = lock.bitstreamBufferPtr;
    out->size = lock.bitstreamSizeInBytes;
    out->timestamp = lock.outputTimeStamp;
    out->duration = lock.outputDuration;
    out->keyframe = lock.pictureType == NV_ENC_PIC_TYPE_IDR;
    return out->data && out->size ? 0 : INVALID_STATE;
}

int noni_nvenc_release(Encoder *enc, uint32_t index)
{
    if (index >= SLOT_COUNT || !enc->slots[index].locked)
        return INVALID_STATE;
    Slot *slot = &enc->slots[index];
    NVENCSTATUS status = enc->api.nvEncUnlockBitstream(enc->session, slot->bitstream);
    if (status != NV_ENC_SUCCESS)
        return status;
    slot->locked = 0;
    status = enc->api.nvEncUnmapInputResource(enc->session, slot->mapped);
    if (status == NV_ENC_SUCCESS)
        slot->mapped = NULL;
    return status;
}
