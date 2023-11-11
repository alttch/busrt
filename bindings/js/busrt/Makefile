all:
	npm run build

bump:
	npm version --no-git-tag-version patch

pub: all upload

upload:
	npm publish --access public
