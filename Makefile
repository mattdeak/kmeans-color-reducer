.PHONY: build serve

build:
	cd rust && wasm-pack build --target web
	cd ..
	rm -r pkg
	mv rust/pkg .
	rm pkg/.gitignore pkg/*.ts

serve: build
	python3 -m http.server

all: serve
