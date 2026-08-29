#include "d3d12_harness.hpp"

#include "shader_sources.hpp"
#include "system_monitor.hpp"

#include <d3dcompiler.h>

#include <algorithm>
#include <chrono>
#include <cstring>
#include <cmath>
#include <iomanip>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>

namespace game_load::win32 {
namespace {

using Microsoft::WRL::ComPtr;
using clock = std::chrono::steady_clock;

constexpr wchar_t kWindowClass[] = L"ResponseConsoleGameLoadHarnessWindow";
constexpr std::uint64_t kPressureBufferBytes = 4ULL * 1024ULL * 1024ULL;
constexpr std::uint64_t kVramChunkBytes = 64ULL * 1024ULL * 1024ULL;

void require(HRESULT result, const char* operation) {
  if (SUCCEEDED(result)) return;
  std::ostringstream message;
  message << operation << " failed (HRESULT 0x" << std::hex
          << static_cast<std::uint32_t>(result) << ')';
  throw std::runtime_error(message.str());
}

ComPtr<ID3DBlob> compile_shader(const char* source, const char* entry,
                                const char* target) {
  UINT flags = D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3;
#if defined(_DEBUG)
  flags = D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_DEBUG |
          D3DCOMPILE_SKIP_OPTIMIZATION;
#endif
  ComPtr<ID3DBlob> bytecode;
  ComPtr<ID3DBlob> diagnostics;
  const HRESULT result = D3DCompile(source, std::strlen(source), "embedded.hlsl",
                                    nullptr, nullptr, entry, target, flags, 0,
                                    &bytecode, &diagnostics);
  if (FAILED(result)) {
    const auto text = diagnostics
                          ? std::string(static_cast<const char*>(diagnostics->GetBufferPointer()),
                                        diagnostics->GetBufferSize())
                          : std::string("no shader diagnostics");
    throw std::runtime_error("shader compilation failed: " + text);
  }
  return bytecode;
}

D3D12_HEAP_PROPERTIES heap_properties(D3D12_HEAP_TYPE type) {
  D3D12_HEAP_PROPERTIES properties{};
  properties.Type = type;
  properties.CPUPageProperty = D3D12_CPU_PAGE_PROPERTY_UNKNOWN;
  properties.MemoryPoolPreference = D3D12_MEMORY_POOL_UNKNOWN;
  properties.CreationNodeMask = 1;
  properties.VisibleNodeMask = 1;
  return properties;
}

D3D12_RESOURCE_DESC buffer_desc(std::uint64_t bytes,
                                D3D12_RESOURCE_FLAGS flags = D3D12_RESOURCE_FLAG_NONE) {
  D3D12_RESOURCE_DESC description{};
  description.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
  description.Alignment = 0;
  description.Width = bytes;
  description.Height = 1;
  description.DepthOrArraySize = 1;
  description.MipLevels = 1;
  description.Format = DXGI_FORMAT_UNKNOWN;
  description.SampleDesc = {1, 0};
  description.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
  description.Flags = flags;
  return description;
}

D3D12_RESOURCE_BARRIER transition(ID3D12Resource* resource,
                                  D3D12_RESOURCE_STATES before,
                                  D3D12_RESOURCE_STATES after) {
  D3D12_RESOURCE_BARRIER barrier{};
  barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
  barrier.Transition.pResource = resource;
  barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
  barrier.Transition.StateBefore = before;
  barrier.Transition.StateAfter = after;
  return barrier;
}

std::string driver_version(IDXGIAdapter* adapter) {
  LARGE_INTEGER version{};
  if (FAILED(adapter->CheckInterfaceSupport(__uuidof(IDXGIDevice), &version))) {
    return "unavailable";
  }
  const auto packed = static_cast<std::uint64_t>(version.QuadPart);
  std::ostringstream out;
  out << ((packed >> 48U) & 0xffffU) << '.' << ((packed >> 32U) & 0xffffU)
      << '.' << ((packed >> 16U) & 0xffffU) << '.' << (packed & 0xffffU);
  return out.str();
}

std::wstring profile_title(LoadProfile profile) {
  const auto text = to_string(profile);
  return std::wstring(text.begin(), text.end());
}

}  // namespace

D3d12Harness::D3d12Harness(WorkloadConfig config) : config_(std::move(config)) {}

D3d12Harness::~D3d12Harness() { cleanup(); }

LRESULT CALLBACK D3d12Harness::window_proc(HWND window, UINT message,
                                           WPARAM wparam, LPARAM lparam) {
  D3d12Harness* self = reinterpret_cast<D3d12Harness*>(
      GetWindowLongPtrW(window, GWLP_USERDATA));
  if (message == WM_NCCREATE) {
    const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
    self = static_cast<D3d12Harness*>(create->lpCreateParams);
    SetWindowLongPtrW(window, GWLP_USERDATA,
                      reinterpret_cast<LONG_PTR>(self));
  }
  if (self != nullptr) {
    if (message == WM_KEYDOWN && wparam == VK_ESCAPE) {
      self->escape_pressed_ = true;
      return 0;
    }
    if (message == WM_CLOSE) {
      self->window_closed_ = true;
      DestroyWindow(window);
      return 0;
    }
    if (message == WM_DESTROY) {
      self->window_ = nullptr;
      PostQuitMessage(0);
      return 0;
    }
  }
  return DefWindowProcW(window, message, wparam, lparam);
}

void D3d12Harness::initialize_window() {
  const auto instance = GetModuleHandleW(nullptr);
  WNDCLASSEXW window_class{};
  window_class.cbSize = sizeof(window_class);
  window_class.style = CS_HREDRAW | CS_VREDRAW;
  window_class.lpfnWndProc = &D3d12Harness::window_proc;
  window_class.hInstance = instance;
  window_class.hCursor = LoadCursorW(nullptr, IDC_ARROW);
  window_class.hbrBackground = reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
  window_class.lpszClassName = kWindowClass;
  if (RegisterClassExW(&window_class) == 0 &&
      GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
    throw std::runtime_error("RegisterClassExW failed");
  }

  constexpr DWORD style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
  RECT rectangle{0, 0, static_cast<LONG>(config_.width),
                 static_cast<LONG>(config_.height)};
  AdjustWindowRectEx(&rectangle, style, FALSE, 0);
  const int width = rectangle.right - rectangle.left;
  const int height = rectangle.bottom - rectangle.top;
  const int x = std::max(0, (GetSystemMetrics(SM_CXSCREEN) - width) / 2);
  const int y = std::max(0, (GetSystemMetrics(SM_CYSCREEN) - height) / 2);

  window_ = CreateWindowExW(
      0, kWindowClass, L"Response Console Game-Load Harness", style, x, y, width,
      height, nullptr, nullptr, instance, this);
  if (window_ == nullptr) throw std::runtime_error("CreateWindowExW failed");
  ShowWindow(window_, SW_SHOW);
  UpdateWindow(window_);
}

void D3d12Harness::initialize_device() {
  require(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory_)), "CreateDXGIFactory2");

  for (UINT index = 0;; ++index) {
    ComPtr<IDXGIAdapter4> candidate;
    if (factory_->EnumAdapterByGpuPreference(index,
            DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IID_PPV_ARGS(&candidate)) ==
        DXGI_ERROR_NOT_FOUND) {
      break;
    }
    DXGI_ADAPTER_DESC3 description{};
    candidate->GetDesc3(&description);
    if ((description.Flags & DXGI_ADAPTER_FLAG3_SOFTWARE) != 0) continue;
    if (SUCCEEDED(D3D12CreateDevice(candidate.Get(), D3D_FEATURE_LEVEL_12_0,
                                    __uuidof(ID3D12Device), nullptr))) {
      adapter_ = candidate;
      break;
    }
  }
  if (!adapter_) throw std::runtime_error("No hardware D3D12 feature-level 12_0 adapter found");
  require(adapter_.As(&budget_adapter_), "Query IDXGIAdapter3");
  require(D3D12CreateDevice(adapter_.Get(), D3D_FEATURE_LEVEL_12_0,
                            IID_PPV_ARGS(&device_)), "D3D12CreateDevice");

  D3D12_COMMAND_QUEUE_DESC queue_description{};
  queue_description.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
  queue_description.Priority = D3D12_COMMAND_QUEUE_PRIORITY_NORMAL;
  require(device_->CreateCommandQueue(&queue_description,
                                       IID_PPV_ARGS(&queue_)),
          "CreateCommandQueue");
  require(queue_->GetTimestampFrequency(&gpu_timestamp_frequency_),
          "GetTimestampFrequency");
}

void D3d12Harness::initialize_swap_chain() {
  DXGI_SWAP_CHAIN_DESC1 description{};
  description.Width = config_.width;
  description.Height = config_.height;
  description.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
  description.Stereo = FALSE;
  description.SampleDesc = {1, 0};
  description.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
  description.BufferCount = kFrameCount;
  description.Scaling = DXGI_SCALING_STRETCH;
  description.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
  description.AlphaMode = DXGI_ALPHA_MODE_IGNORE;

  ComPtr<IDXGISwapChain1> swap_chain;
  require(factory_->CreateSwapChainForHwnd(queue_.Get(), window_, &description,
                                            nullptr, nullptr, &swap_chain),
          "CreateSwapChainForHwnd");
  require(factory_->MakeWindowAssociation(window_, DXGI_MWA_NO_ALT_ENTER),
          "MakeWindowAssociation");
  require(swap_chain.As(&swap_chain_), "Query IDXGISwapChain3");
  current_back_buffer_ = swap_chain_->GetCurrentBackBufferIndex();
}

void D3d12Harness::initialize_pipeline() {
  auto scene_vertex = compile_shader(shaders::scene, "vsMain", "vs_5_1");
  auto scene_pixel = compile_shader(shaders::scene, "psMain", "ps_5_1");
  auto pressure = compile_shader(shaders::pressure, "main", "cs_5_1");

  D3D12_ROOT_PARAMETER scene_parameter{};
  scene_parameter.ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
  scene_parameter.Constants = {0, 0, 4};
  scene_parameter.ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
  D3D12_ROOT_SIGNATURE_DESC scene_signature{};
  scene_signature.NumParameters = 1;
  scene_signature.pParameters = &scene_parameter;
  scene_signature.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

  ComPtr<ID3DBlob> signature_blob;
  ComPtr<ID3DBlob> signature_error;
  require(D3D12SerializeRootSignature(&scene_signature, D3D_ROOT_SIGNATURE_VERSION_1,
                                      &signature_blob, &signature_error),
          "Serialize scene root signature");
  require(device_->CreateRootSignature(0, signature_blob->GetBufferPointer(),
                                        signature_blob->GetBufferSize(),
                                        IID_PPV_ARGS(&scene_root_signature_)),
          "Create scene root signature");

  D3D12_GRAPHICS_PIPELINE_STATE_DESC graphics{};
  graphics.pRootSignature = scene_root_signature_.Get();
  graphics.VS = {scene_vertex->GetBufferPointer(), scene_vertex->GetBufferSize()};
  graphics.PS = {scene_pixel->GetBufferPointer(), scene_pixel->GetBufferSize()};
  graphics.BlendState.AlphaToCoverageEnable = FALSE;
  graphics.BlendState.IndependentBlendEnable = FALSE;
  const D3D12_RENDER_TARGET_BLEND_DESC render_target_blend{
      FALSE, FALSE, D3D12_BLEND_ONE, D3D12_BLEND_ZERO, D3D12_BLEND_OP_ADD,
      D3D12_BLEND_ONE, D3D12_BLEND_ZERO, D3D12_BLEND_OP_ADD,
      D3D12_LOGIC_OP_NOOP, D3D12_COLOR_WRITE_ENABLE_ALL};
  for (auto& target : graphics.BlendState.RenderTarget) target = render_target_blend;
  graphics.SampleMask = UINT_MAX;
  graphics.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
  graphics.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
  graphics.RasterizerState.FrontCounterClockwise = FALSE;
  graphics.RasterizerState.DepthBias = D3D12_DEFAULT_DEPTH_BIAS;
  graphics.RasterizerState.DepthBiasClamp = D3D12_DEFAULT_DEPTH_BIAS_CLAMP;
  graphics.RasterizerState.SlopeScaledDepthBias = D3D12_DEFAULT_SLOPE_SCALED_DEPTH_BIAS;
  graphics.RasterizerState.DepthClipEnable = TRUE;
  graphics.RasterizerState.MultisampleEnable = FALSE;
  graphics.RasterizerState.AntialiasedLineEnable = FALSE;
  graphics.RasterizerState.ForcedSampleCount = 0;
  graphics.RasterizerState.ConservativeRaster = D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF;
  graphics.DepthStencilState.DepthEnable = FALSE;
  graphics.DepthStencilState.StencilEnable = FALSE;
  graphics.InputLayout = {nullptr, 0};
  graphics.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
  graphics.NumRenderTargets = 1;
  graphics.RTVFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
  graphics.SampleDesc = {1, 0};
  require(device_->CreateGraphicsPipelineState(&graphics,
                                                IID_PPV_ARGS(&scene_pipeline_)),
          "Create scene pipeline");

  D3D12_DESCRIPTOR_RANGE uav_range{};
  uav_range.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_UAV;
  uav_range.NumDescriptors = 1;
  uav_range.BaseShaderRegister = 0;
  uav_range.RegisterSpace = 0;
  uav_range.OffsetInDescriptorsFromTableStart = 0;
  D3D12_ROOT_PARAMETER compute_parameters[2]{};
  compute_parameters[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
  compute_parameters[0].DescriptorTable = {1, &uav_range};
  compute_parameters[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
  compute_parameters[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
  compute_parameters[1].Constants = {0, 0, 2};
  compute_parameters[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
  D3D12_ROOT_SIGNATURE_DESC compute_signature{};
  compute_signature.NumParameters = 2;
  compute_signature.pParameters = compute_parameters;
  compute_signature.Flags = D3D12_ROOT_SIGNATURE_FLAG_NONE;
  signature_blob.Reset();
  signature_error.Reset();
  require(D3D12SerializeRootSignature(&compute_signature, D3D_ROOT_SIGNATURE_VERSION_1,
                                      &signature_blob, &signature_error),
          "Serialize compute root signature");
  require(device_->CreateRootSignature(0, signature_blob->GetBufferPointer(),
                                        signature_blob->GetBufferSize(),
                                        IID_PPV_ARGS(&compute_root_signature_)),
          "Create compute root signature");

  D3D12_COMPUTE_PIPELINE_STATE_DESC compute{};
  compute.pRootSignature = compute_root_signature_.Get();
  compute.CS = {pressure->GetBufferPointer(), pressure->GetBufferSize()};
  require(device_->CreateComputePipelineState(&compute,
                                               IID_PPV_ARGS(&compute_pipeline_)),
          "Create compute pipeline");
}

void D3d12Harness::initialize_frame_resources() {
  D3D12_DESCRIPTOR_HEAP_DESC rtv_description{};
  rtv_description.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
  rtv_description.NumDescriptors = kFrameCount;
  require(device_->CreateDescriptorHeap(&rtv_description,
                                         IID_PPV_ARGS(&rtv_heap_)),
          "Create RTV heap");
  rtv_descriptor_size_ = device_->GetDescriptorHandleIncrementSize(
      D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
  auto rtv = rtv_heap_->GetCPUDescriptorHandleForHeapStart();
  for (std::uint32_t index = 0; index < kFrameCount; ++index) {
    require(swap_chain_->GetBuffer(index, IID_PPV_ARGS(&back_buffers_[index])),
            "Get swap-chain buffer");
    device_->CreateRenderTargetView(back_buffers_[index].Get(), nullptr, rtv);
    rtv.ptr += rtv_descriptor_size_;
    require(device_->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
                                             IID_PPV_ARGS(&frames_[index].allocator)),
            "Create command allocator");
  }

  D3D12_DESCRIPTOR_HEAP_DESC uav_description{};
  uav_description.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
  uav_description.NumDescriptors = 1;
  uav_description.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
  require(device_->CreateDescriptorHeap(&uav_description,
                                         IID_PPV_ARGS(&uav_heap_)),
          "Create UAV heap");

  const auto default_heap = heap_properties(D3D12_HEAP_TYPE_DEFAULT);
  const auto pressure_desc = buffer_desc(kPressureBufferBytes,
                                         D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
  require(device_->CreateCommittedResource(&default_heap, D3D12_HEAP_FLAG_NONE,
                                            &pressure_desc,
                                            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                                            nullptr,
                                            IID_PPV_ARGS(&pressure_buffer_)),
          "Create pressure buffer");
  D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};
  uav.ViewDimension = D3D12_UAV_DIMENSION_BUFFER;
  uav.Format = DXGI_FORMAT_R32_TYPELESS;
  uav.Buffer.NumElements = static_cast<UINT>(kPressureBufferBytes / 4);
  uav.Buffer.Flags = D3D12_BUFFER_UAV_FLAG_RAW;
  device_->CreateUnorderedAccessView(pressure_buffer_.Get(), nullptr, &uav,
                                     uav_heap_->GetCPUDescriptorHandleForHeapStart());

  D3D12_QUERY_HEAP_DESC query{};
  query.Type = D3D12_QUERY_HEAP_TYPE_TIMESTAMP;
  query.Count = kFrameCount * 4;
  require(device_->CreateQueryHeap(&query, IID_PPV_ARGS(&timestamp_heap_)),
          "Create timestamp query heap");
  const auto readback_heap = heap_properties(D3D12_HEAP_TYPE_READBACK);
  const auto readback_desc = buffer_desc(kFrameCount * 4 * sizeof(std::uint64_t));
  require(device_->CreateCommittedResource(&readback_heap, D3D12_HEAP_FLAG_NONE,
                                            &readback_desc,
                                            D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
                                            IID_PPV_ARGS(&timestamp_readback_)),
          "Create timestamp readback");
  D3D12_RANGE no_read{0, 0};
  require(timestamp_readback_->Map(0, &no_read,
                                   reinterpret_cast<void**>(&mapped_timestamps_)),
          "Map timestamp readback");

  require(device_->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
                                      frames_[0].allocator.Get(), nullptr,
                                      IID_PPV_ARGS(&command_list_)),
          "Create command list");
  require(command_list_->Close(), "Close initial command list");
  require(device_->CreateFence(0, D3D12_FENCE_FLAG_NONE,
                                IID_PPV_ARGS(&fence_)),
          "Create fence");
  fence_event_ = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  if (fence_event_ == nullptr) throw std::runtime_error("CreateEventW failed");
}

void D3d12Harness::allocate_synthetic_vram(RunResult& result) {
  const auto default_heap = heap_properties(D3D12_HEAP_TYPE_DEFAULT);
  std::uint64_t remaining = result.vram_allocation.approved_bytes;
  while (remaining > 0) {
    const auto snapshot = query_video_memory(budget_adapter_.Get());
    const auto live_headroom = snapshot.budget_bytes > snapshot.current_usage_bytes
                                   ? snapshot.budget_bytes - snapshot.current_usage_bytes
                                   : 0;
    if (!snapshot.available || live_headroom <= mib(config_.min_vram_headroom_mib)) {
      result.warnings.emplace_back(
          "Synthetic VRAM allocation stopped early to preserve live DXGI headroom");
      break;
    }
    const auto safe_live = live_headroom - mib(config_.min_vram_headroom_mib);
    const auto chunk = std::min({remaining, kVramChunkBytes, safe_live});
    if (chunk < 64 * 1024) break;
    const auto resource_desc = buffer_desc(chunk);
    ComPtr<ID3D12Resource> resource;
    const HRESULT created = device_->CreateCommittedResource(
        &default_heap, D3D12_HEAP_FLAG_NONE, &resource_desc,
        D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&resource));
    if (FAILED(created)) {
      result.warnings.emplace_back(
          "D3D12 declined a bounded synthetic VRAM chunk; continuing with less pressure");
      break;
    }
    synthetic_vram_.push_back(std::move(resource));
    result.committed_vram_bytes += chunk;
    remaining -= chunk;
  }
}

bool D3d12Harness::wait_for_frame(std::uint32_t slot, RunResult& result) {
  const auto value = frames_[slot].fence_value;
  if (value == 0 || fence_->GetCompletedValue() >= value) return true;
  if (FAILED(fence_->SetEventOnCompletion(value, fence_event_))) {
    result.stop_reason = SafetyAbort::device_removed;
    result.stop_detail = "Could not arm the D3D12 fence completion event";
    return false;
  }
  const auto wait = WaitForSingleObject(fence_event_, config_.gpu_fence_timeout_ms);
  if (wait != WAIT_OBJECT_0) {
    result.stop_reason = SafetyAbort::gpu_timeout;
    result.stop_detail = "GPU fence wait exceeded the configured safety timeout";
    result.safety_events.push_back(result.stop_detail);
    return false;
  }
  return true;
}

bool D3d12Harness::flush_gpu(RunResult& result) {
  if (!queue_ || !fence_) return true;
  const auto value = next_fence_value_++;
  if (FAILED(queue_->Signal(fence_.Get(), value)) ||
      FAILED(fence_->SetEventOnCompletion(value, fence_event_))) {
    return false;
  }
  if (WaitForSingleObject(fence_event_, config_.gpu_fence_timeout_ms) != WAIT_OBJECT_0) {
    result.safety_events.emplace_back("Final GPU flush timed out; resources released without a blocking retry");
    return false;
  }
  return true;
}

void D3d12Harness::record_frame(std::uint32_t slot, std::uint64_t frame_number,
                                std::uint32_t compute_groups) {
  auto& frame = frames_[slot];
  require(frame.allocator->Reset(), "Reset command allocator");
  require(command_list_->Reset(frame.allocator.Get(), nullptr), "Reset command list");
  const UINT query_base = slot * 4;
  command_list_->EndQuery(timestamp_heap_.Get(), D3D12_QUERY_TYPE_TIMESTAMP,
                          query_base);

  command_list_->SetPipelineState(compute_pipeline_.Get());
  command_list_->SetComputeRootSignature(compute_root_signature_.Get());
  ID3D12DescriptorHeap* heaps[] = {uav_heap_.Get()};
  command_list_->SetDescriptorHeaps(1, heaps);
  command_list_->SetComputeRootDescriptorTable(
      0, uav_heap_->GetGPUDescriptorHandleForHeapStart());
  const std::uint32_t pressure_constants[2] = {
      static_cast<std::uint32_t>(frame_number * 747796405ULL + 2891336453ULL),
      static_cast<std::uint32_t>(kPressureBufferBytes / 4)};
  command_list_->SetComputeRoot32BitConstants(1, 2, pressure_constants, 0);
  command_list_->EndQuery(timestamp_heap_.Get(), D3D12_QUERY_TYPE_TIMESTAMP,
                          query_base + 1);
  // D3D12 caps one dispatch dimension at 65,535 groups. Split larger calibrated
  // workloads into deterministic batches so typical/heavy profiles can still reach
  // their frame-duty target without increasing shader complexity or using a spin.
  std::uint32_t groups_remaining = std::max(1U, compute_groups);
  while (groups_remaining > 0) {
    const auto batch = std::min(65'535U, groups_remaining);
    command_list_->Dispatch(batch, 1, 1);
    groups_remaining -= batch;
  }
  command_list_->EndQuery(timestamp_heap_.Get(), D3D12_QUERY_TYPE_TIMESTAMP,
                          query_base + 2);
  D3D12_RESOURCE_BARRIER uav_barrier{};
  uav_barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_UAV;
  uav_barrier.UAV.pResource = pressure_buffer_.Get();
  command_list_->ResourceBarrier(1, &uav_barrier);

  auto to_render = transition(back_buffers_[slot].Get(),
                              D3D12_RESOURCE_STATE_PRESENT,
                              D3D12_RESOURCE_STATE_RENDER_TARGET);
  command_list_->ResourceBarrier(1, &to_render);
  D3D12_CPU_DESCRIPTOR_HANDLE rtv = rtv_heap_->GetCPUDescriptorHandleForHeapStart();
  rtv.ptr += static_cast<SIZE_T>(slot) * rtv_descriptor_size_;
  constexpr float clear[4] = {0.008f, 0.012f, 0.014f, 1.0f};
  command_list_->ClearRenderTargetView(rtv, clear, 0, nullptr);
  command_list_->OMSetRenderTargets(1, &rtv, FALSE, nullptr);
  const D3D12_VIEWPORT viewport{0.0f, 0.0f, static_cast<float>(config_.width),
                               static_cast<float>(config_.height), 0.0f, 1.0f};
  const D3D12_RECT scissor{0, 0, static_cast<LONG>(config_.width),
                           static_cast<LONG>(config_.height)};
  command_list_->RSSetViewports(1, &viewport);
  command_list_->RSSetScissorRects(1, &scissor);
  command_list_->SetPipelineState(scene_pipeline_.Get());
  command_list_->SetGraphicsRootSignature(scene_root_signature_.Get());
  command_list_->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
  const std::uint32_t scene_constants[4] = {
      static_cast<std::uint32_t>(frame_number), config_.width, config_.height,
      static_cast<std::uint32_t>(config_.profile)};
  command_list_->SetGraphicsRoot32BitConstants(0, 4, scene_constants, 0);
  command_list_->DrawInstanced(3, 1, 0, 0);
  auto to_present = transition(back_buffers_[slot].Get(),
                               D3D12_RESOURCE_STATE_RENDER_TARGET,
                               D3D12_RESOURCE_STATE_PRESENT);
  command_list_->ResourceBarrier(1, &to_present);
  command_list_->EndQuery(timestamp_heap_.Get(), D3D12_QUERY_TYPE_TIMESTAMP,
                          query_base + 3);
  command_list_->ResolveQueryData(
      timestamp_heap_.Get(), D3D12_QUERY_TYPE_TIMESTAMP, query_base, 4,
      timestamp_readback_.Get(), static_cast<UINT64>(query_base) * sizeof(std::uint64_t));
  require(command_list_->Close(), "Close frame command list");
}

void D3d12Harness::consume_queries(std::uint32_t slot,
                                   ComputeCalibrator& calibrator,
                                   SampleSeries& gpu_frame,
                                   SampleSeries& gpu_compute,
                                   RunResult& result) {
  auto& frame = frames_[slot];
  if (!frame.has_queries || gpu_timestamp_frequency_ == 0) return;
  const auto* values = mapped_timestamps_ + slot * 4;
  if (values[3] >= values[0] && values[2] >= values[1]) {
    const double factor = 1000.0 / static_cast<double>(gpu_timestamp_frequency_);
    const double frame_ms = static_cast<double>(values[3] - values[0]) * factor;
    const double compute_ms = static_cast<double>(values[2] - values[1]) * factor;
    gpu_frame.add(frame_ms);
    gpu_compute.add(compute_ms);
    const auto calibrated_groups = calibrator.next(
        {compute_ms, frame.submitted_compute_groups});
    (void)calibrated_groups;
    ++result.frames_measured;
  }
  frame.has_queries = false;
}

bool D3d12Harness::pump_messages() {
  MSG message{};
  while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
    if (message.message == WM_QUIT) {
      window_closed_ = true;
      return false;
    }
    TranslateMessage(&message);
    DispatchMessageW(&message);
  }
  return !window_closed_;
}

void D3d12Harness::update_window_title(const RunResult& result,
                                       const SampleSeries& qpc_frame,
                                       const SampleSeries& gpu_frame,
                                       const SampleSeries& gpu_compute) {
  if (window_ == nullptr) return;
  std::wostringstream title;
  title << L"Response Console Game Load | " << profile_title(config_.profile)
        << L" | frame " << result.frames_presented << L" | QPC "
        << std::fixed << std::setprecision(2) << qpc_frame.mean() << L" ms | GPU "
        << gpu_frame.mean() << L" ms | compute " << gpu_compute.mean()
        << L" ms | Esc stops";
  SetWindowTextW(window_, title.str().c_str());
}

RunResult D3d12Harness::run() {
  RunResult result;
  result.config = config_;
  result.dry_run = false;
  result.harness_version = GAME_LOAD_VERSION;
  result.started_utc = utc_now_iso8601();
  result.operating_system = query_operating_system();
  result.cpu_name = query_cpu_name();
  result.logical_cpu_count = std::max(1U, std::thread::hardware_concurrency());
  result.power_settings_changed = false;

  SampleSeries qpc_frame;
  SampleSeries gpu_frame;
  SampleSeries gpu_compute;
  SampleSeries temperatures(4096);
  SampleSeries vram_usage(4096);
  SampleSeries available_ram(4096);
  ComputeCalibrator calibrator(target_compute_ms(config_), 1, 8'000'000);

  try {
    initialize_device();
    DXGI_ADAPTER_DESC3 adapter_description{};
    adapter_->GetDesc3(&adapter_description);
    result.adapter.name = wide_to_utf8(adapter_description.Description);
    result.adapter.vendor_id = adapter_description.VendorId;
    result.adapter.device_id = adapter_description.DeviceId;
    result.adapter.dedicated_video_memory_bytes = adapter_description.DedicatedVideoMemory;
    result.adapter.dedicated_system_memory_bytes = adapter_description.DedicatedSystemMemory;
    result.adapter.shared_system_memory_bytes = adapter_description.SharedSystemMemory;
    result.adapter.timestamp_frequency_hz = gpu_timestamp_frequency_;
    result.adapter.driver_version = driver_version(adapter_.Get());

    result.initial_system_memory = query_system_memory();
    result.initial_video_memory = query_video_memory(budget_adapter_.Get());
    std::string thermal_warning;
    auto temperature = query_acpi_temperature_c(&thermal_warning);
    if (!temperature && !thermal_warning.empty()) result.warnings.push_back(thermal_warning);
    SafetySnapshot preflight{temperature, result.initial_system_memory,
                             result.initial_video_memory};
    const auto preflight_decision = evaluate_safety(config_, preflight);
    if (preflight_decision.abort != SafetyAbort::none) {
      result.stop_reason = preflight_decision.abort;
      result.stop_detail = "Preflight: " + preflight_decision.detail;
      result.safety_events.push_back(result.stop_detail);
      result.ended_utc = utc_now_iso8601();
      cleanup();
      return result;
    }

    initialize_window();
    initialize_swap_chain();
    initialize_pipeline();
    initialize_frame_resources();

    // Do not start pressure until the visible scene and every safety/measurement
    // resource are ready. Allocation is rechecked against live budgets chunk-by-chunk.
    result.vram_allocation = decide_vram_allocation(config_,
                                                     query_video_memory(budget_adapter_.Get()));
    result.ram_allocation = decide_ram_allocation(config_, query_system_memory());
    allocate_synthetic_vram(result);
    const auto cpu_workers = resolve_cpu_threads(config_, result.logical_cpu_count);
    if (!cpu_ram_load_.start(config_.cpu_duty_percent, cpu_workers,
                             result.ram_allocation.approved_bytes)) {
      result.warnings.emplace_back(
          "Windows declined the bounded RAM commit; CPU pressure continued without it");
    }
    result.committed_ram_bytes = cpu_ram_load_.committed_ram_bytes();

    markers_.event(L"GameLoadHarness.Workload.Begin profile=" +
                   profile_title(config_.profile));
    const auto run_start = clock::now();
    auto last_safety_poll = run_start - std::chrono::milliseconds(config_.safety_poll_ms);
    auto last_thermal_poll = run_start - std::chrono::seconds(2);
    auto last_title_update = run_start;
    LARGE_INTEGER qpc_frequency{};
    QueryPerformanceFrequency(&qpc_frequency);

    while (result.stop_reason == SafetyAbort::none) {
      if (!pump_messages()) {
        result.stop_reason = SafetyAbort::window_closed;
        result.stop_detail = "The harness window was closed";
        break;
      }
      if (escape_pressed_) {
        result.stop_reason = SafetyAbort::user_requested;
        result.stop_detail = "Escape requested a safe stop";
        break;
      }
      if (clock::now() - run_start >= std::chrono::seconds(config_.duration_seconds)) {
        result.stop_reason = SafetyAbort::duration_complete;
        result.stop_detail = "Configured duration completed";
        result.completed = true;
        break;
      }

      current_back_buffer_ = swap_chain_->GetCurrentBackBufferIndex();
      if (!wait_for_frame(current_back_buffer_, result)) break;
      consume_queries(current_back_buffer_, calibrator, gpu_frame, gpu_compute, result);

      LARGE_INTEGER qpc_begin{}, qpc_end{};
      QueryPerformanceCounter(&qpc_begin);
      const auto groups = calibrator.current();
      {
        EtwScope frame_scope(markers_, L"GameLoadHarness.Frame",
                             result.frames_presented);
        record_frame(current_back_buffer_, result.frames_presented, groups);
        ID3D12CommandList* lists[] = {command_list_.Get()};
        queue_->ExecuteCommandLists(1, lists);
        const HRESULT presented = swap_chain_->Present(config_.vsync ? 1 : 0, 0);
        if (FAILED(presented)) {
          result.stop_reason = SafetyAbort::device_removed;
          result.stop_detail = "Swap-chain Present failed, likely due to a device reset";
          result.safety_events.push_back(result.stop_detail);
          break;
        }
        const auto fence_value = next_fence_value_++;
        if (FAILED(queue_->Signal(fence_.Get(), fence_value))) {
          result.stop_reason = SafetyAbort::device_removed;
          result.stop_detail = "D3D12 queue signaling failed";
          break;
        }
        auto& submitted = frames_[current_back_buffer_];
        submitted.fence_value = fence_value;
        submitted.submitted_compute_groups = groups;
        submitted.has_queries = true;
      }
      QueryPerformanceCounter(&qpc_end);
      if (qpc_frequency.QuadPart > 0) {
        qpc_frame.add(static_cast<double>(qpc_end.QuadPart - qpc_begin.QuadPart) *
                      1000.0 / static_cast<double>(qpc_frequency.QuadPart));
      }
      ++result.frames_presented;

      const auto now = clock::now();
      if (now - last_safety_poll >=
          std::chrono::milliseconds(config_.safety_poll_ms)) {
        last_safety_poll = now;
        auto system_memory = query_system_memory();
        auto video_memory = query_video_memory(budget_adapter_.Get());
        if (system_memory.available) {
          available_ram.add(static_cast<double>(system_memory.available_physical_bytes) /
                            static_cast<double>(mib(1)));
        }
        if (video_memory.available) {
          vram_usage.add(static_cast<double>(video_memory.current_usage_bytes) /
                         static_cast<double>(mib(1)));
        }
        if (now - last_thermal_poll >= std::chrono::seconds(2)) {
          last_thermal_poll = now;
          temperature = query_acpi_temperature_c();
          if (temperature) temperatures.add(*temperature);
        }
        SafetySnapshot snapshot{temperature, system_memory, video_memory};
        snapshot.device_removed = FAILED(device_->GetDeviceRemovedReason());
        const auto safety = evaluate_safety(config_, snapshot);
        if (safety.abort != SafetyAbort::none) {
          result.stop_reason = safety.abort;
          result.stop_detail = safety.detail;
          result.safety_events.push_back(safety.detail);
          markers_.event(L"GameLoadHarness.SafetyAbort " +
                         std::wstring(to_string(safety.abort).begin(),
                                      to_string(safety.abort).end()),
                         2, 0x2);
          break;
        }
      }
      if (now - last_title_update >= std::chrono::seconds(1)) {
        last_title_update = now;
        update_window_title(result, qpc_frame, gpu_frame, gpu_compute);
      }

      // Present(1) is not a reliable pacing primitive under every compositor,
      // remote-session, and capture configuration. An explicit deterministic
      // deadline keeps GPU duty tied to the requested frame budget and prevents an
      // unexpectedly unthrottled swap chain from multiplying load.
      const auto frame_deadline = run_start + std::chrono::duration_cast<clock::duration>(
          std::chrono::duration<double>(
              static_cast<double>(result.frames_presented) /
              static_cast<double>(config_.target_fps)));
      if (clock::now() < frame_deadline) std::this_thread::sleep_until(frame_deadline);
    }

    const bool device_healthy = device_ && SUCCEEDED(device_->GetDeviceRemovedReason());
    if (device_healthy && flush_gpu(result)) {
      for (std::uint32_t slot = 0; slot < kFrameCount; ++slot) {
        consume_queries(slot, calibrator, gpu_frame, gpu_compute, result);
      }
    }
    markers_.event(L"GameLoadHarness.Workload.End reason=" +
                   std::wstring(to_string(result.stop_reason).begin(),
                                to_string(result.stop_reason).end()));
  } catch (const std::exception& exception) {
    if (result.stop_reason == SafetyAbort::none) {
      result.stop_reason = result.frames_presented == 0
                               ? SafetyAbort::initialization_failure
                               : SafetyAbort::internal_error;
      result.stop_detail = exception.what();
      result.safety_events.push_back(result.stop_detail);
    }
  }

  result.final_compute_dispatches = calibrator.current();
  result.qpc_frame_ms = summarize(qpc_frame);
  result.gpu_frame_ms = summarize(gpu_frame);
  result.gpu_compute_ms = summarize(gpu_compute);
  result.temperature_c = summarize(temperatures);
  result.vram_usage_mib = summarize(vram_usage);
  result.available_ram_mib = summarize(available_ram);
  result.ended_utc = utc_now_iso8601();
  cleanup();
  return result;
}

void D3d12Harness::cleanup() noexcept {
  cpu_ram_load_.stop();
  if (timestamp_readback_ && mapped_timestamps_ != nullptr) {
    D3D12_RANGE no_write{0, 0};
    timestamp_readback_->Unmap(0, &no_write);
    mapped_timestamps_ = nullptr;
  }
  synthetic_vram_.clear();
  pressure_buffer_.Reset();
  timestamp_readback_.Reset();
  timestamp_heap_.Reset();
  for (auto& back_buffer : back_buffers_) back_buffer.Reset();
  for (auto& frame : frames_) {
    frame.allocator.Reset();
    frame.fence_value = 0;
    frame.has_queries = false;
  }
  command_list_.Reset();
  scene_pipeline_.Reset();
  compute_pipeline_.Reset();
  scene_root_signature_.Reset();
  compute_root_signature_.Reset();
  rtv_heap_.Reset();
  uav_heap_.Reset();
  fence_.Reset();
  swap_chain_.Reset();
  queue_.Reset();
  device_.Reset();
  budget_adapter_.Reset();
  adapter_.Reset();
  factory_.Reset();
  if (fence_event_ != nullptr) {
    CloseHandle(fence_event_);
    fence_event_ = nullptr;
  }
  if (window_ != nullptr) {
    SetWindowLongPtrW(window_, GWLP_USERDATA, 0);
    DestroyWindow(window_);
    window_ = nullptr;
  }
}

}  // namespace game_load::win32
