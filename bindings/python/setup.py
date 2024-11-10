from setuptools import setup, find_packages

setup(
    name="bitkitcore",
    version="0.1.0",
    packages=find_packages(),
    package_data={
        "bitkitcore": ["*.so", "*.dylib", "*.dll"],
    },
    install_requires=[],
    author="Synonym",
    author_email="",
    description="Bitcoin & Lightning invoice parsing library",
    long_description=open("README.md").read(),
    long_description_content_type="text/markdown",
    url="",
    classifiers=[
        "Programming Language :: Python :: 3",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
    ],
    python_requires=">=3.6",
)
