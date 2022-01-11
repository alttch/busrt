VERSION=0.0.5

all:
	@echo "what do you want to build today?"

ver:
	find . -type f -name "*.py" -exec sed -i "s/^__version__ = .*/__version__ = '${VERSION}'/g" {} \;

d: build

build:
	rm -rf dist build elbus.egg-info
	python3 setup.py sdist

pub: d pub-pypi

pub-pypi: upload-pypi

upload-pypi:
	twine upload dist/*

clean:
	rm -rf dist build elbus.egg-info
