#pragma once

#include "cpu_ram_load.hpp"
#include "etw_markers.hpp"
#include "harness_types.hpp"

#include <Windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace game_load::win32 {

class D3d12Harness final {
 public:
  explicit D3d12Harness(WorkloadConfig config);
  ~D3d12Harness();
  D3d12Harness(const D3d12Harness&) = delete;
  D3d12Harness& operator=(const D3d12Harness&) = delete;

  RunResult run();

 private:
  static constexpr std::uint32_t kFrameCount = 3;
  struct FrameState {
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
    std::uint64_t fence_value = 0;
    std::uint32_t submitted_compute_groups = 1;
    bool has_queries = false;
  };

  static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam,
                                      LPARAM lparam);
  void initialize_window();
  void initialize_device();
  void initialize_pipeline();
  void initialize_swap_chain();
  void initialize_frame_resources();
  void allocate_synthetic_vram(RunResult& result);
  bool wait_for_frame(std::uint32_t slot, RunResult& result);
  bool flush_gpu(RunResult& result);
  void record_frame(std::uint32_t slot, std::uint64_t frame_number,
                    std::uint32_t compute_groups);
  void consume_queries(std::uint32_t slot, ComputeCalibrator& calibrator,
                       SampleSeries& gpu_frame, SampleSeries& gpu_compute,
                       RunResult& result);
  bool pump_messages();
  void update_window_title(const RunResult& result, const SampleSeries& qpc_frame,
                           const SampleSeries& gpu_frame,
                           const SampleSeries& gpu_compute);
  void cleanup() noexcept;

  WorkloadConfig config_;
  HWND window_ = nullptr;
  bool window_closed_ = false;
  bool escape_pressed_ = false;
  HANDLE fence_event_ = nullptr;
  std::uint64_t next_fence_value_ = 1;
  std::uint32_t current_back_buffer_ = 0;
  std::uint32_t rtv_descriptor_size_ = 0;
  std::uint64_t gpu_timestamp_frequency_ = 0;

  Microsoft::WRL::ComPtr<IDXGIFactory6> factory_;
  Microsoft::WRL::ComPtr<IDXGIAdapter4> adapter_;
  Microsoft::WRL::ComPtr<IDXGIAdapter3> budget_adapter_;
  Microsoft::WRL::ComPtr<ID3D12Device> device_;
  Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue_;
  Microsoft::WRL::ComPtr<IDXGISwapChain3> swap_chain_;
  Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> command_list_;
  Microsoft::WRL::ComPtr<ID3D12Fence> fence_;
  Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> rtv_heap_;
  Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> uav_heap_;
  Microsoft::WRL::ComPtr<ID3D12RootSignature> scene_root_signature_;
  Microsoft::WRL::ComPtr<ID3D12RootSignature> compute_root_signature_;
  Microsoft::WRL::ComPtr<ID3D12PipelineState> scene_pipeline_;
  Microsoft::WRL::ComPtr<ID3D12PipelineState> compute_pipeline_;
  Microsoft::WRL::ComPtr<ID3D12Resource> pressure_buffer_;
  Microsoft::WRL::ComPtr<ID3D12QueryHeap> timestamp_heap_;
  Microsoft::WRL::ComPtr<ID3D12Resource> timestamp_readback_;
  std::uint64_t* mapped_timestamps_ = nullptr;
  std::array<Microsoft::WRL::ComPtr<ID3D12Resource>, kFrameCount> back_buffers_;
  std::array<FrameState, kFrameCount> frames_;
  std::vector<Microsoft::WRL::ComPtr<ID3D12Resource>> synthetic_vram_;

  CpuRamLoad cpu_ram_load_;
  EtwMarkers markers_;
};

}  // namespace game_load::win32
