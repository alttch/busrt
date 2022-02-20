__version__ = '0.0.12'

import setuptools

with open('README.md', 'r') as fh:
    long_description = fh.read()

setuptools.setup(
    name='elbus',
    version=__version__,
    author='Bohemia Automation / Altertech',
    author_email='div@altertech.com',
    description='python client for elbus (sync)',
    long_description=long_description,
    long_description_content_type='text/markdown',
    url='https://github.com/alttch/elbus',
    packages=setuptools.find_packages(),
    license='MIT',
    classifiers=('Programming Language :: Python :: 3',
                 'License :: OSI Approved :: MIT License',
                 'Topic :: Software Development :: Libraries',
                 'Topic :: Communications'),
)
