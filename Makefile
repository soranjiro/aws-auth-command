switch-readme:
	@if [ -z "$(LANG)" ]; then echo "Usage: make switch-readme LANG=ja|en"; exit 1; fi
	cp README.$(LANG).md README.md
	@echo "README set to $(LANG)"
