all: decomposition_report.pdf

decomposition_report.pdf: decomposition-report/main.pdf
	ln -s decomposition-report/main.pdf decomposition_report.pdf

decomposition-report/main.pdf: decomposition-report/main.tex
	pdflatex -output-directory=decomposition-report decomposition-report/main.tex

.PHONY: all clean

clean:
	rm -f decomposition_report.pdf
	rm -f decomposition-report/main.{pdf,aux,log}
