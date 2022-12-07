all: decomposition_report.pdf code_progress.pdf

decomposition_report.pdf: decomposition-report/main.pdf
	cp decomposition-report/main.pdf decomposition_report.pdf

decomposition-report/main.pdf: decomposition-report/main.tex
	pdflatex -output-directory=decomposition-report decomposition-report/main.tex

code_progress.pdf: code-progress/main.pdf
	cp code-progress/main.pdf code_progress.pdf

code-progress/main.pdf: code-progress/main.tex
	pdflatex -output-directory=code-progress code-progress/main.tex

.PHONY: all clean

clean:
	rm -f decomposition_report.pdf
	rm -f decomposition-report/main.{pdf,aux,log}
	rm -f code_progress.pdf
	rm -f code_progress/main.{pdf,aux,log}
