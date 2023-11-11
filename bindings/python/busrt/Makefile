VERSION=0.1.2

all:
	@echo "what do you want to build today?"

ver:
	find . -type f -name "*.py" -exec sed -i "s/^__version__ = .*/__version__ = '${VERSION}'/g" {} \;

d: build

build:
	rm -rf dist build busrt.egg-info
	python3 setup.py sdist

pub: d pub-pypi pub-anaconda

pub-pypi: upload-pypi

upload-pypi:
	twine upload dist/*

pub-anaconda:
	/opt/anaconda3/bin/anaconda upload -u bohemia-automation dist/*.tar.gz

clean:
	rm -rf dist build busrt.egg-info
