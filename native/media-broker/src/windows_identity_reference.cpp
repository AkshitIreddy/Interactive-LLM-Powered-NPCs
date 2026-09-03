#ifdef _WIN32

#include "windows_identity_reference.hpp"

#include <Windows.h>
#include <ShObjIdl.h>
#include <bcrypt.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <array>
#include <algorithm>
#include <span>
#include <thread>

namespace npc::media::windows {
namespace {

using Microsoft::WRL::ComPtr;

[[nodiscard]] std::string sha256_hex(const std::span<const std::byte> bytes) {
    BCRYPT_ALG_HANDLE algorithm{};
    BCRYPT_HASH_HANDLE hash{};
    DWORD object_size{}, copied{};
    std::array<std::byte, 32> digest{};
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) != 0 ||
        BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                          reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size), &copied, 0) != 0) {
        if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
        return {};
    }
    std::vector<std::byte> object(object_size);
    const bool accepted = BCryptCreateHash(
        algorithm, &hash, reinterpret_cast<PUCHAR>(object.data()), object_size, nullptr, 0, 0) == 0 &&
        BCryptHashData(hash,
                       const_cast<PUCHAR>(reinterpret_cast<const UCHAR*>(bytes.data())),
                       static_cast<ULONG>(bytes.size()), 0) == 0 &&
        BCryptFinishHash(hash, reinterpret_cast<PUCHAR>(digest.data()),
                         static_cast<ULONG>(digest.size()), 0) == 0;
    if (hash) BCryptDestroyHash(hash);
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (!accepted) return {};
    constexpr char hex[] = "0123456789abcdef";
    std::string result;
    result.reserve(64U);
    for (const auto byte : digest) {
        const auto value = std::to_integer<std::uint8_t>(byte);
        result.push_back(hex[value >> 4U]);
        result.push_back(hex[value & 0x0fU]);
    }
    return result;
}

[[nodiscard]] bool read_bounded_file(const wchar_t* path,
                                     std::vector<std::byte>& encoded,
                                     Failure& failure) {
    constexpr std::uint64_t maximum_encoded_bytes = 32U * 1024U * 1024U;
    HANDLE file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
                              FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    LARGE_INTEGER length{};
    if (file == INVALID_HANDLE_VALUE || !GetFileSizeEx(file, &length) || length.QuadPart <= 0 ||
        static_cast<std::uint64_t>(length.QuadPart) > maximum_encoded_bytes) {
        if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
        failure = {FailureDomain::capture, FailureCode::invalid_geometry, false,
                   "Identity reference encoded bytes exceed the 32 MiB product bound"};
        return false;
    }
    encoded.resize(static_cast<std::size_t>(length.QuadPart));
    std::size_t offset{};
    while (offset < encoded.size()) {
        DWORD read{};
        if (!ReadFile(file, encoded.data() + offset,
                      static_cast<DWORD>((std::min)(encoded.size() - offset,
                                                    static_cast<std::size_t>(1U << 20U))),
                      &read, nullptr) || read == 0U) {
            CloseHandle(file);
            encoded.clear();
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity reference file could not be read exactly"};
            return false;
        }
        offset += read;
    }
    CloseHandle(file);
    return true;
}

[[nodiscard]] bool pick_and_decode_sta(DecodedIdentityReference& decoded, Failure& failure) {
    ComPtr<IFileOpenDialog> dialog;
    HRESULT hr = CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER,
                                  IID_PPV_ARGS(&dialog));
    constexpr std::array filters{COMDLG_FILTERSPEC{L"PNG or JPEG image", L"*.png;*.jpg;*.jpeg"}};
    if (SUCCEEDED(hr)) hr = dialog->SetFileTypes(static_cast<UINT>(filters.size()), filters.data());
    if (SUCCEEDED(hr)) hr = dialog->SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST |
                                               FOS_PATHMUSTEXIST | FOS_DONTADDTORECENT |
                                               FOS_NOCHANGEDIR);
    if (SUCCEEDED(hr)) hr = dialog->Show(nullptr);
    if (hr == HRESULT_FROM_WIN32(ERROR_CANCELLED)) {
        failure = {FailureDomain::capture, FailureCode::access_denied, false,
                   "Identity reference selection was cancelled"};
        return false;
    }
    ComPtr<IShellItem> item;
    if (SUCCEEDED(hr)) hr = dialog->GetResult(&item);
    PWSTR selected_path{};
    if (SUCCEEDED(hr)) hr = item->GetDisplayName(SIGDN_FILESYSPATH, &selected_path);
    if (FAILED(hr) || !selected_path) {
        if (selected_path) CoTaskMemFree(selected_path);
        failure = {FailureDomain::capture, FailureCode::access_denied, false,
                   "Identity reference picker did not return a local file"};
        return false;
    }
    std::vector<std::byte> encoded;
    const bool read = read_bounded_file(selected_path, encoded, failure);
    CoTaskMemFree(selected_path);
    selected_path = nullptr;
    if (!read) return false;

    ComPtr<IWICImagingFactory> factory;
    ComPtr<IWICStream> stream;
    ComPtr<IWICBitmapDecoder> decoder;
    ComPtr<IWICBitmapFrameDecode> frame;
    ComPtr<IWICFormatConverter> converter;
    hr = CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
                          IID_PPV_ARGS(&factory));
    if (SUCCEEDED(hr)) hr = factory->CreateStream(&stream);
    if (SUCCEEDED(hr)) hr = stream->InitializeFromMemory(
        reinterpret_cast<BYTE*>(encoded.data()), static_cast<DWORD>(encoded.size()));
    if (SUCCEEDED(hr)) hr = factory->CreateDecoderFromStream(
        stream.Get(), nullptr, WICDecodeMetadataCacheOnLoad, &decoder);
    GUID container{};
    if (SUCCEEDED(hr)) hr = decoder->GetContainerFormat(&container);
    if (SUCCEEDED(hr) && container != GUID_ContainerFormatPng &&
        container != GUID_ContainerFormatJpeg) hr = WINCODEC_ERR_BADIMAGE;
    if (SUCCEEDED(hr)) hr = decoder->GetFrame(0U, &frame);
    UINT width{}, height{};
    if (SUCCEEDED(hr)) hr = frame->GetSize(&width, &height);
    const auto byte_length = static_cast<std::uint64_t>(width) * height * 4U;
    if (FAILED(hr) || width == 0U || height == 0U || width > 8192U || height > 8192U ||
        byte_length == 0U || byte_length > 64U * 1024U * 1024U) {
        failure = {FailureDomain::capture, FailureCode::invalid_geometry, false,
                   "Identity reference dimensions exceed 8192 pixels or 64 MiB"};
        return false;
    }
    hr = factory->CreateFormatConverter(&converter);
    if (SUCCEEDED(hr)) hr = converter->Initialize(
        frame.Get(), GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, nullptr, 0.0,
        WICBitmapPaletteTypeCustom);
    std::vector<std::byte> pixels(static_cast<std::size_t>(byte_length));
    const auto stride = width * 4U;
    if (SUCCEEDED(hr)) hr = converter->CopyPixels(
        nullptr, stride, static_cast<UINT>(pixels.size()),
        reinterpret_cast<BYTE*>(pixels.data()));
    if (FAILED(hr)) {
        failure = {FailureDomain::capture, FailureCode::backend_unavailable, false,
                   "Identity reference could not be normalized to BGRA8"};
        return false;
    }
    const auto asset_digest = sha256_hex(encoded);
    std::fill(encoded.begin(), encoded.end(), std::byte{});
    encoded.clear();
    if (asset_digest.size() != 64U) {
        failure = {FailureDomain::capture, FailureCode::internal_error, false,
                   "Identity reference source digest could not be computed"};
        return false;
    }
    decoded = {std::move(pixels), width, height, stride,
               container == GUID_ContainerFormatPng ? "image/png" : "image/jpeg",
               asset_digest};
    return true;
}

} // namespace

bool pick_and_decode_identity_reference(DecodedIdentityReference& decoded, Failure& failure) {
    decoded = {};
    bool accepted{};
    std::thread picker([&] {
        const HRESULT initialized = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED |
                                                            COINIT_DISABLE_OLE1DDE);
        if (FAILED(initialized)) {
            failure = {FailureDomain::capture, FailureCode::backend_unavailable, false,
                       "Identity reference picker could not initialize its STA"};
            return;
        }
        accepted = pick_and_decode_sta(decoded, failure);
        CoUninitialize();
    });
    picker.join();
    return accepted;
}

} // namespace npc::media::windows

#endif
