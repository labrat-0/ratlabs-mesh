#include "rasterizer.h"
// CPU-only build: the CUDA kernel is absent, so GPU tensors are rejected.
std::vector<torch::Tensor> rasterize_image_gpu(torch::Tensor V, torch::Tensor F, torch::Tensor D,
    int width, int height, float occlusion_truncation, int use_depth_prior) {
  TORCH_CHECK(false, "custom_rasterizer was built CPU-only; pass CPU tensors");
}
