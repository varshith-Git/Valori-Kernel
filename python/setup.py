from setuptools import setup, find_packages
from setuptools_rust import Binding, RustExtension

setup(
    name="valori",
    version="0.1.0",
    packages=find_packages(),
    rust_extensions=[
        RustExtension("valori.valori_ffi", path="../ffi/Cargo.toml", binding=Binding.PyO3),
    ],
    install_requires=[
        "requests>=2.25.0",
    ],
    setup_requires=["setuptools-rust>=1.5.0"],
    zip_safe=False,
)
