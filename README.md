# PhotoStep ⚡

**محرر صور بأسلوب فوتوشوب، بلغة Rust خالص — سريع، عملي، متكامل.**

مستوحى من [photocraft](https://github.com/storytold/photocraft) لكن بهدف مختلف:
أخف، أسرع في البناء والتشغيل، وكود واضح يمكن فهمه وتطويره.

## لماذا PhotoStep أسرع؟

- **بنية بسيطة:** ملفات قليلة (`core / ops / io / app`) بدل 24 crate — بناء في ثوانٍ.
- **توازي حقيقي:** كل الفلاتر والتعديلات تعمل بـ `rayon` على كل الأنوية.
- **Gaussian سريع:** تقريب 3×Box separable بدل convolution ثقيل (نفس خدعة Photoshop).
- **مسار سريع للـ Normal blend** بدون حسابات float زائدة.
- **رفع Texture واحد** فقط عند تغيّر الصورة، لا كل فريم.

## المزايا

- 🗂 **طبقات:** إضافة/حذف/تكرار/دمج/تسطيح، شفافية، 13 blend modes، إظهار/إخفاء
- 🎨 **فرشاة وممحاة** ناعمة الحواف + دلو تعبئة + قطارة لون + تحديد مستطيل
- 📊 **تعديلات:** سطوع، تباين، تشبع، تعريض، Invert، رمادي، Threshold، Posterize، Auto-Contrast، Hue، Vibrance، Levels، Color Balance
- ✨ **فلاتر:** Blur، Sharpen (unsharp mask)، Edges، Emboss، Pixelate، Noise، Vignette
- 🔄 **تحويلات:** تدوير 90°، قلب أفقي/عمودي، Undo/Redo (30 خطوة)
- 💾 **صيغ:** PNG/JPG/TIFF/BMP/WebP/GIF/QOI + مشروع `.pstep` (JSON)
- ⌨️ **CLI headless** للمعالجة الدفعية (مثل أوامر photocraft)

## التشغيل

### 1) ثبّت Rust (مرة واحدة)

```powershell
winget install -e --id Rustlang.Rustup
# ثم أغلق الطرفية وأعد فتحها
rustup toolchain install stable
```

### 2) شغّل الواجهة

```powershell
cd photostep
cargo run --release
```

### 3) المعالجة بدون واجهة (batch)

```powershell
cargo run --release -- --input in.png --out out.png --op "brightness:20" --op "contrast:25" --op "blur:4" --op sharpen:1.2
cargo run --release -- --input photo.jpg --out gray.png --op grayscale --op "vignette:0.6"
```

عمليات مدعومة: `brightness:N contrast:N invert grayscale threshold:N posterize:N exposure:N vibrance:N saturate:X hue:deg blur:R sharpen:A edge emboss pixelate:N noise:N vignette:X autocontrast fliph flipv rot90`

## اختصارات

`Ctrl+Z` تراجع · `Ctrl+Y` إعادة · أدوات: فرشاة B، ممحاة E، تعبئة G، قطارة I، تحريك V

## البنية

```
src/main.rs  → CLI + إطلاق egui
src/core.rs  → Document/Layer/Blend/History + اختبارات
src/ops.rs   → تعديلات + فلاتر + تحويلات (rayon)
src/io.rs    → تحميل/حفظ صور + .pstep
src/app.rs   → واجهة فوتوشوب (tools/canvas/layers/adjustments)
```

## الخطوة التالية (لو تريد التوسع)

- Tile-based CoW مثل photocraft للوحات الضخمة 8K
- دعم PSD عبر crate `psd`
- GPU compositor عبر `wgpu`
- فرشاة بضغط القلم + طبقات ضبط (adjustment layers)

MIT OR Apache-2.0 — مستقل تماماً، لا علاقة له بـ Adobe.
