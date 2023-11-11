__version__ = '0.1.3'

import setuptools

with open('README.md', 'r') as fh:
    long_description = fh.read()

setuptools.setup(
    name='busrt_async',
    version=__version__,
    author='Bohemia Automation / Altertech',
    author_email='div@altertech.com',
    description='Python client for BUS/RT (async)',
    long_description=long_description,
    long_description_content_type='text/markdown',
    url='https://github.com/alttch/busrt',
    packages=setuptools.find_packages(),
    license='MIT',
    classifiers=('Programming Language :: Python :: 3',
                 'License :: OSI Approved :: MIT License',
                 'Topic :: Software Development :: Libraries',
                 'Topic :: Communications'),
)
