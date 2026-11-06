// Native macOS bridge for markitdown-rs.
//
// Uses only frameworks shipped with the OS:
//   * Vision  -> VNRecognizeTextRequest (revision 3, accurate), Apple Neural Engine
//   * PDFKit  -> render scanned PDF pages to bitmaps before OCR
//
// Compiled by build.rs with ARC. Exposes a plain C ABI so Rust needs no
// Objective-C runtime or objc2 crates.

#import <Foundation/Foundation.h>
#import <Vision/Vision.h>
#import <AppKit/AppKit.h>
#import <PDFKit/PDFKit.h>

#include <math.h>
#include <stdlib.h>
#include <string.h>

// ---------------------------------------------------------------------------
// Language selection
//
// IMPORTANT: on recent macOS, Chinese accuracy collapses unless a Chinese
// identifier appears FIRST in recognitionLanguages. Passing ["en-US",
// "zh-Hans"] or relying on automatic detection effectively disables Chinese.
// We therefore keep the caller's order, and filter against the languages the
// active revision actually supports (unknown identifiers make Vision throw).
// ---------------------------------------------------------------------------
static NSArray<NSString *> *MDPickLanguages(VNRecognizeTextRequest *request, const char *csv) {
    if (csv == NULL || csv[0] == '\0') return nil;
    NSString *raw = [NSString stringWithUTF8String:csv];
    if (raw == nil) return nil;

    NSMutableArray<NSString *> *wanted = [NSMutableArray array];
    for (NSString *part in [raw componentsSeparatedByString:@","]) {
        NSString *s = [part stringByTrimmingCharactersInSet:
                       [NSCharacterSet whitespaceAndNewlineCharacterSet]];
        if (s.length > 0) [wanted addObject:s];
    }
    if (wanted.count == 0) return nil;

    NSArray<NSString *> *supported = nil;
    if (@available(macOS 11.0, *)) {
        NSError *err = nil;
        supported = [VNRecognizeTextRequest
            supportedRecognitionLanguagesForTextRecognitionLevel:request.recognitionLevel
                                                        revision:request.revision
                                                           error:&err];
    }

    NSMutableArray<NSString *> *picked = [NSMutableArray array];
    for (NSString *lang in wanted) {
        if (supported == nil || [supported containsObject:lang]) [picked addObject:lang];
    }
    return picked.count > 0 ? picked : nil;
}

int md_vision_available(void) {
    if (@available(macOS 11.0, *)) return 1;
    return 0;
}

// Returns a malloc'd NUL-terminated UTF-8 string; release with md_free().
// Returns NULL when the framework could not run (never on a supported Mac).
char *md_vision_ocr(const unsigned char *bytes, size_t length, const char *languages) {
    @autoreleasepool {
        if (bytes == NULL || length == 0) return NULL;

        NSData *data = [NSData dataWithBytesNoCopy:(void *)bytes
                                            length:length
                                      freeWhenDone:NO];
        NSImage *image = [[NSImage alloc] initWithData:data];
        if (image == nil) return NULL;
        CGImageRef cg = [image CGImageForProposedRect:NULL context:nil hints:nil];
        if (cg == NULL) return NULL;

        VNImageRequestHandler *handler =
            [[VNImageRequestHandler alloc] initWithCGImage:cg options:@{}];
        VNRecognizeTextRequest *request = [[VNRecognizeTextRequest alloc] init];
        request.recognitionLevel = VNRequestTextRecognitionLevelAccurate;
        request.usesLanguageCorrection = YES;

        if (@available(macOS 11.0, *)) {
            request.revision = VNRecognizeTextRequestRevision3;
            NSArray<NSString *> *langs = MDPickLanguages(request, languages);
            if (langs != nil) request.recognitionLanguages = langs;
        }

        NSError *error = nil;
        if (![handler performRequests:@[request] error:&error]) return NULL;

        // Restore reading order: top-to-bottom, then left-to-right on a line.
        NSArray<VNRecognizedTextObservation *> *observations = [request.results
            sortedArrayUsingComparator:^NSComparisonResult(
                VNRecognizedTextObservation *a, VNRecognizedTextObservation *b) {
                CGFloat ay = a.boundingBox.origin.y;
                CGFloat by = b.boundingBox.origin.y;
                if (fabs((double)(ay - by)) > 0.012) {
                    return ay > by ? NSOrderedAscending : NSOrderedDescending;
                }
                CGFloat ax = a.boundingBox.origin.x;
                CGFloat bx = b.boundingBox.origin.x;
                if (ax < bx) return NSOrderedAscending;
                if (ax > bx) return NSOrderedDescending;
                return NSOrderedSame;
            }];

        NSMutableString *text = [NSMutableString string];
        for (VNRecognizedTextObservation *obs in observations) {
            VNRecognizedText *top = [[obs topCandidates:1] firstObject];
            if (top != nil) {
                [text appendString:top.string];
                [text appendString:@"\n"];
            }
        }
        const char *utf8 = [text UTF8String];
        return strdup(utf8 != NULL ? utf8 : "");
    }
}

// ---------------------------------------------------------------------------
// PDFKit page rendering (used only when a PDF has no usable text layer).
// ---------------------------------------------------------------------------
size_t md_pdf_page_count(const unsigned char *bytes, size_t length) {
    @autoreleasepool {
        if (bytes == NULL || length == 0) return 0;
        NSData *data = [NSData dataWithBytesNoCopy:(void *)bytes
                                            length:length
                                      freeWhenDone:NO];
        PDFDocument *doc = [[PDFDocument alloc] initWithData:data];
        return doc != nil ? (size_t)[doc pageCount] : 0;
    }
}

// Renders page `pageIndex` (0-based) to PNG. Returns a malloc'd buffer.
unsigned char *md_pdf_render_page(const unsigned char *bytes, size_t length,
                                  size_t pageIndex, double dpi, size_t *outLength) {
    @autoreleasepool {
        if (outLength != NULL) *outLength = 0;
        if (bytes == NULL || length == 0) return NULL;

        NSData *data = [NSData dataWithBytesNoCopy:(void *)bytes
                                            length:length
                                      freeWhenDone:NO];
        PDFDocument *doc = [[PDFDocument alloc] initWithData:data];
        if (doc == nil || pageIndex >= (size_t)[doc pageCount]) return NULL;
        PDFPage *page = [doc pageAtIndex:pageIndex];
        if (page == nil) return NULL;

        NSRect bounds = [page boundsForBox:kPDFDisplayBoxMediaBox];
        double scale = dpi / 72.0;
        if (scale < 0.5) scale = 0.5;
        if (scale > 6.0) scale = 6.0;
        NSSize target = NSMakeSize(fmax(bounds.size.width * scale, 64.0),
                                   fmax(bounds.size.height * scale, 64.0));

        NSImage *thumb = [page thumbnailOfSize:target forBox:kPDFDisplayBoxMediaBox];
        if (thumb == nil) return NULL;
        NSData *tiff = [thumb TIFFRepresentation];
        if (tiff == nil) return NULL;
        NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithData:tiff];
        if (rep == nil) return NULL;
        NSData *png = [rep representationUsingType:NSBitmapImageFileTypePNG
                                        properties:@{}];
        if (png == nil || png.length == 0) return NULL;

        unsigned char *buf = (unsigned char *)malloc(png.length);
        if (buf == NULL) return NULL;
        memcpy(buf, png.bytes, png.length);
        if (outLength != NULL) *outLength = (size_t)png.length;
        return buf;
    }
}

void md_free(void *ptr) {
    if (ptr != NULL) free(ptr);
}
