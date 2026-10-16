from setuptools import setup, find_packages
from torch.utils.cpp_extension import BuildExtension, CppExtension
setup(name="custom_rasterizer", version="0.1+cpu", packages=find_packages(),
      ext_modules=[CppExtension("custom_rasterizer_kernel",
          ["src/rasterizer.cpp", "src/grid_neighbor.cpp", "src/gpu_stub.cpp"], extra_compile_args=["-O3", "-fopenmp"], extra_link_args=["-fopenmp"])],
      cmdclass={"build_ext": BuildExtension})
