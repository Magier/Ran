copy-armory:
	rm -rf src/armory/builtin/*
	cp -a armory/TTPs/. src/armory/builtin/


.PHONY: build
build: copy-armory
	pnpm --prefix frontend build
	cp -r frontend/build/. src/api/static/
	cd src && go build -o ../dist/ran .