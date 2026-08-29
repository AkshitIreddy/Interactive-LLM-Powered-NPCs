RWByteAddressBuffer PressureBuffer : register(u0);

cbuffer PressureConstants : register(b0) {
  uint seed;
  uint wordCount;
};

[numthreads(64, 1, 1)]
void main(uint3 dispatchThreadId : SV_DispatchThreadID) {
  uint value = seed ^ (dispatchThreadId.x * 0x9e3779b9u);
  [unroll(64)]
  for (uint i = 0; i < 64; ++i) {
    value ^= value >> 16;
    value *= 0x7feb352du;
    value ^= value >> 15;
    value *= 0x846ca68bu;
    value ^= value >> 16;
  }
  const uint index = (dispatchThreadId.x * 4u) % max(wordCount * 4u, 4u);
  PressureBuffer.Store(index, value);
}
