cbuffer SceneConstants : register(b0) {
  uint frameIndex;
  uint surfaceWidth;
  uint surfaceHeight;
  uint profileKey;
};

struct VertexOutput {
  float4 position : SV_Position;
  float2 uv : TEXCOORD0;
};

VertexOutput vsMain(uint vertexId : SV_VertexID) {
  VertexOutput output;
  const float2 position = float2((vertexId << 1) & 2, vertexId & 2);
  output.uv = position;
  output.position = float4(position * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
  return output;
}

float hash21(float2 p) {
  p = frac(p * float2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return frac(p.x * p.y);
}

float lineBand(float value, float center, float width) {
  return 1.0 - smoothstep(width, width * 1.8, abs(value - center));
}

float sdCircle(float2 position, float radius) {
  return length(position) - radius;
}

float4 psMain(VertexOutput input) : SV_Target {
  const float2 resolution = float2(surfaceWidth, surfaceHeight);
  const float aspect = resolution.x / max(resolution.y, 1.0);
  float2 uv = input.uv;
  float2 p = (uv - 0.5) * float2(aspect, 1.0);
  const float time = (float)frameIndex / 60.0;

  // Smoked graphite base with deterministic scan texture.
  float3 color = lerp(float3(0.018, 0.026, 0.029), float3(0.038, 0.052, 0.056), uv.y);
  color += (hash21(floor(uv * resolution / 2.0)) - 0.5) * 0.008;
  color += 0.010 * sin((uv.y * surfaceHeight + frameIndex * 0.25) * 0.25);

  // A synthetic city/game scene gives capture tools stable large, medium and fine detail.
  const float horizon = 0.58;
  color += float3(0.01, 0.04, 0.045) * smoothstep(horizon + 0.1, horizon - 0.1, uv.y);
  for (int tower = 0; tower < 14; ++tower) {
    const float key = hash21(float2(tower, profileKey));
    const float x = (tower + 0.5) / 14.0;
    const float halfWidth = 0.018 + key * 0.020;
    const float top = horizon - 0.08 - key * 0.24;
    const float body = step(abs(uv.x - x), halfWidth) * step(top, uv.y) * step(uv.y, horizon);
    color = lerp(color, float3(0.025, 0.041, 0.045), body * 0.92);
    const float windows = step(0.82, hash21(floor(float2((uv.x - x) * 900.0, uv.y * 500.0))));
    color += body * windows * float3(0.03, 0.34, 0.31);
  }

  // Perspective floor grid with a slow deterministic camera drift.
  if (uv.y > horizon) {
    const float depth = 1.0 / max(uv.y - horizon, 0.01);
    const float gridX = abs(frac((p.x * depth + time * 0.08) * 0.45) - 0.5);
    const float gridY = abs(frac(depth * 0.10 - time * 0.12) - 0.5);
    const float grid = smoothstep(0.055, 0.0, min(gridX, gridY));
    color += grid * float3(0.015, 0.16, 0.145) * saturate((uv.y - horizon) * 2.0);
  }

  // Center subject silhouette and face anchor exercise stable compositing/capture regions.
  float2 subject = p - float2(0.06 * sin(time * 0.37), -0.025);
  const float head = 1.0 - smoothstep(0.0, 0.012, sdCircle(subject - float2(0, -0.12), 0.105));
  const float shoulders = 1.0 - smoothstep(0.0, 0.015,
      length((subject - float2(0, 0.16)) * float2(0.62, 1.0)) - 0.28);
  const float silhouette = saturate(max(head, shoulders));
  color = lerp(color, float3(0.012, 0.019, 0.022), silhouette * 0.97);
  const float faceRim = 1.0 - smoothstep(0.012, 0.025,
      abs(sdCircle(subject - float2(0, -0.12), 0.105)));
  color += faceRim * float3(0.03, 0.45, 0.39) * 0.35;

  // Response Spine: seven real pipeline stages represented in the synthetic scene.
  const float spineX = -aspect * 0.5 + 0.105;
  const float spine = lineBand(p.x, spineX, 0.0025) * step(-0.31, p.y) * step(p.y, 0.31);
  color += spine * float3(0.04, 0.34, 0.31);
  for (int stage = 0; stage < 7; ++stage) {
    const float stageY = -0.27 + stage * 0.09;
    const float pulse = 0.60 + 0.40 * sin(time * 2.0 - stage * 0.7);
    const float node = 1.0 - smoothstep(0.008, 0.014,
        length(p - float2(spineX, stageY)));
    color += node * float3(0.08, 0.72, 0.63) * pulse;
  }

  // Framing corners and luminance ramps make capture corruption visually obvious.
  const float edgeX = lineBand(abs(p.x), aspect * 0.5 - 0.025, 0.0015);
  const float edgeY = lineBand(abs(p.y), 0.475, 0.0015);
  color += (edgeX + edgeY) * float3(0.04, 0.22, 0.20);
  color *= 1.0 - 0.38 * dot(uv - 0.5, uv - 0.5);
  return float4(saturate(color), 1.0);
}
