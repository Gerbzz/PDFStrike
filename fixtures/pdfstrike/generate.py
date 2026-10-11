"""Generate self-authored independent PDFStrike test documents with ReportLab/pypdf.

This is test fixture tooling, not application or UI code.
"""

from pathlib import Path
from tempfile import TemporaryDirectory

from pypdf import PdfReader, PdfWriter
from reportlab.lib.colors import HexColor
from reportlab.lib.pagesizes import letter
from reportlab.pdfgen import canvas


OUT = Path(__file__).resolve().parent


def packet() -> None:
    with TemporaryDirectory() as tmp:
        base = Path(tmp) / "packet-base.pdf"
        pdf = canvas.Canvas(str(base), pagesize=letter, pageCompression=1)
        pdf.setTitle("PDFStrike independent packet fixture")
        for number, color in enumerate(("#2456A6", "#2F7660", "#985E2E", "#683F8C"), 1):
            pdf.setFillColor(HexColor(color))
            pdf.roundRect(54, 650, 504, 80, 12, fill=1, stroke=0)
            pdf.setFillColorRGB(1, 1, 1)
            pdf.setFont("Helvetica-Bold", 30)
            pdf.drawString(75, 680, f"Packet page A{number}")
            pdf.setFillColorRGB(0.1, 0.15, 0.2)
            pdf.setFont("Helvetica", 13)
            pdf.drawString(72, 605, "Self-authored page content for order, crop and rotation checks.")
            pdf.drawString(72, 580, f"Stable visible page identifier: A{number}")
            pdf.showPage()
        pdf.save()
        reader = PdfReader(str(base))
        writer = PdfWriter()
        writer.append(reader)
        writer.pages[1].rotate(90)
        writer.pages[2].cropbox.lower_left = (36, 36)
        writer.pages[2].cropbox.upper_right = (576, 756)
        with (OUT / "packet-real.pdf").open("wb") as handle:
            writer.write(handle)


def form() -> None:
    dest = OUT / "form-real.pdf"
    pdf = canvas.Canvas(str(dest), pagesize=letter, pageCompression=1)
    pdf.setTitle("PDFStrike independent AcroForm fixture")
    pdf.setFont("Helvetica-Bold", 22)
    pdf.drawString(72, 720, "Review approval")
    pdf.setFont("Helvetica", 12)
    pdf.drawString(72, 685, "Contact name")
    pdf.acroForm.textfield(name="contact.name", tooltip="Contact name", x=72, y=645, width=250, height=26, borderWidth=1, forceBorder=True)
    pdf.drawString(105, 602, "Approved")
    pdf.acroForm.checkbox(name="approved", tooltip="Approved", x=72, y=596, size=20, checked=False, buttonStyle="check", borderWidth=1)
    pdf.showPage()
    pdf.save()


if __name__ == "__main__":
    packet()
    form()
