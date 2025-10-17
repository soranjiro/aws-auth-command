switch-readme:
	@if [ -z "$(LANG)" ]; then echo "Usage: make switch-readme LANG=ja|en"; exit 1; fi
	cp README.$(LANG).md README.md
	@echo "README set to $(LANG)"

.PHONY: package dist clean-dist

dist:
	@bash scripts/package-release.sh

package: dist
	@echo "Release artifacts created under ./dist"

clean-dist:
	rm -rf dist
