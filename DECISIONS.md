# DECISIONS.md — سجل القرارات التقنية والمعمارية (Technical & Architectural Decisions)

## 1. القرارات الفيزيائية والرياضية (Physics & Numerical Stability)
1. **توحيد خطوة التكامل الزمني عند `h = 1/2400` ثانية على جميع مستويات الجودة (`Low` / `Medium` / `High`):**
   - **السبب:** في المحركات المبنية على شبكات النوابض والكتل (Mass-Spring Soft-Body Lattice)، يخضع التكامل شبه الضمني (Semi-Implicit Euler) لشرط استقرار Courant–Friedrichs–Lewy (CFL): يجب أن يكون $\Delta t^2 \cdot rac{\sum k}{m_{	ext{node}}} < 2.6$. خفض التردد إلى `1/1200` ثانية في إعداد `Low` كان يضاعف $\Delta t^2$ أربع مرات، مما يؤدي إلى انفجار القيم إلى `NaN` في أول إطار وتحول مصفوفة الكاميرا إلى شاشة سوداء.
   - **الحل:** تثبيت `h = 1/2400` ثانية في جميع المستويات، مع إضافة دالة `SoftBody::stabilize_for_dt()` التي تفحص القيمة الذاتية لكل عقدة وتقلم صلابة النوابض ومعاملات التخميد تلقائياً لضمان استحالة حدوث انفجار عددي.

2. **القضاء الجذري على اهتزاز العجلات الأمامية في `Mercedes-AMG GT Black Series`:**
   - **السبب:** كان الكود السابق يحسب انحراف المحور الأمامي (`toe_bias`) من الإزاحة اللحظية لمرتكزات التعليق (`local_off[0].z - local_off[1].z`). وبسبب القوة الهائلة لمحرك AMG GT (730 حصان دفع خلفي) وارتداد التعليق، يتعرض الشاسيه لانحناء مرن طبيعي أثناء التسارع والفرملة يتجاوز 4 سم، فيفسره الكود كـ "عطب تصادم" ويضيف زاوية توجيه زائفة (`toe_bias`) تتأرجح يميناً ويساراً في حلقة تغذية راجعة، بالإضافة إلى تغير التوجيه بمقدار خطي حاد (`signum() * rt * dt`).
   - **الحل:**
     - ربط `toe_bias` حصرياً بالتشوه البلاستيكي الدائم الناتج عن الحوادث (`front_plastic > 0.028` في مصفوفة `body.dmg`) بحيث يكون `toe_bias = 0.0` تماماً أثناء القيادة السليمة.
     - تحويل استجابة مقود التوجيه وزوايا العجلات إلى مرشح أسي ناعم (Critically Damped Exponential Filter) مع هندسة Ackermann واقعية وتخميد للسرعة الجانبية الدقيقة.

3. **ضبط التوجيه والمحاور في `Mercedes-AMG One`:**
   - **السبب:** التحقق الهندسي لمجسم `amg_one.cdm` أثبت أن مقدمة السيارة تقع عند `+Z` ومؤخرتها (ذيل القرش والجناح الخلفي) عند `-Z`، والعجلات الأمامية هي `0, 1` عند `z = +1.347m`. تم التأكد من تطابق متجه الأمام (`frame.fwd`) ومصفوفة الرسم (`draw_car`) وحركة التوجيه (`A = +left`, `D = -left`) وهندسة Ackermann بحيث يستجيب التوجيه للأمام والخلف بدقة تامة.

---

## 2. القرارات الرسومية والإضاءة (Graphics, Anti-Aliasing & Lighting — Phase 6)
### ما تم تنفيذه داخل المحرك:
1. **نظام تنعيم الحواف المزدوج (`SMAA + Contrast Adaptive Sharpening CAS` + Edge Alpha AA):**
   - نظراً لأن خط رندر `Macroquad / Miniquad` يعمل على OpenGL ES2 / GLSL 100 دون دعم Multiple Render Targets (MRT) لكتابة Velocity Motion Vectors لكل بكسل، فإن تطبيق TAA المعتمد على G-Buffer Motion Vectors الكامل غير متاح في بنية Miniquad القياسية.
   - بدلاً من ذلك، قمنا ببناء أقوى نظام ممكن ضمن المحرك:
     - **Subpixel Morphological Anti-Aliasing (SMAA 1x)** في شيدر ما بعد المعالجة (`post.frag`) يكتشف الحواف أفقياً وعمودياً وقطرياً ويعيد بناء التدرجات الفرعية للقضاء على التشظي (Pixelation).
     - **FidelityFX-inspired Contrast Adaptive Sharpening (CAS)** يعيد الحدة للتفاصيل الدقيقة بعد التنعيم أو عند استخدام Dynamic Resolution Scaling.
     - **Alpha-to-Coverage Smoothstep AA** داخل `lit.frag` لتنعيم حواف الأسوار الشبكية والأشجار، بالإضافة إلى دعم **Hardware 4x MSAA** عند التشغيل بخيار `--msaa`.
2. **تحسينات الإضاءة الشاملة (`GI Bounce`، `Contact AO`، `9-Tap Poisson PCF Shadows`، `Volumetric Fog`، `ACES Tonemapping`):**
   - ترقية دقة خريطة الظلال إلى `2048x2048` مع مرشح **9-Tap Rotated Poisson Disk PCF** في `shdec.frag` لظلال ناعمة خالية من المربعات.
   - إضافة **Multi-Bounce Hemisphere Global Illumination (GI)** يحسب ارتداد الضوء الدافئ من الأسفلت والأرضية نحو الأسطح السفلية والجانبية للسيارات والمباني.
   - إضافة **Ground Contact & Cavity Ambient Occlusion (AO)** يغمّق مناطق التماس بين الإطارات/الهيكل والأرضية.
   - إضافة **Atmospheric Mie & Rayleigh Volumetric Fog** مع تشتت ضوء الشمس في الأفق وتحويل لوني سينمائي عبر **ACES Filmic Tone Mapping**.

### ما كان خارج قدرة المحرك (Engine Limitations Documented per Phase 6):
- **Hardware Ray-Tracing (RTGI / RTReflections):** غير مدعوم لأن `Miniquad` يستخدم واجهة OpenGL / GLSL 100 ولا يدعم Vulkan Ray-Tracing أو DXR. تم الاستعاضة عنه بانعكاسات سماء وأفق فيزيائية متعددة الطبقات (Dual-Layer Clear Coat Horizon-Occluded Analytical Env Reflections).
- **Motion-Vector G-Buffer TAA / DLSS / FSR2:** يتطلب إخراج Depth + Motion Vectors في G-Buffer متعدد القنوات (MRT) غير مدعوم في `Macroquad 0.4`. تم الاستعاضة عنه بـ `SMAA + CAS Upscaling + Dynamic Resolution + Hardware MSAA`.

---

## 3. إدارة الخرائط والأصول الضخمة واستوديو التصوير (Maps, Lumberyard Bistro & Heavy Assets)
1. **هيكلة الخرائط الثلاث واستوديو التصوير:**
   - **الخريطة 1 (الافتراضية):** `West Coast Urban District` — مبنية مباشرة من ملف `map_file.json` المحدّث، خفيفة جداً وسريعة التحميل وتعمل كبيئة افتراضية.
   - **الخريطة 2:** `West Coast USA` (`west_coast_usa.zip` من تأليف **Ahmed Benali** برخصة **CC-BY 4.0**) — تم وضع الإسناد القانوني الكامل في شاشة `Credits`، وربطها بنافذة تحذير المتطلبات قبل الاختيار.
   - **الخريطة 3:** `Metropolis Megacity (City Sample Inspired)` — نظراً لأن `Unreal Engine City Sample` الأصلية (70–100 GB بصيغة `.uasset`) حصرية لمحرك Unreal Engine 5 ولتقنيتي Nanite/Lumen ولا يمكن تشغيل ملفاتها الخام خارج UE5، قمنا ببناء خريطة `city_sample_megacity.json` الموعودة كشبكة مدينة ضخمة عالية الكثافة (High-Rise Downtown Grid) متوافقة 100% مع محركنا وبحجم مضغوط خفيف جداً دون التضحية بجودة المظهر، مع ربطها بشاشة تحذير العتاد (`16 GB RAM Min / 32 GB Recommended`).
   - **خريطة `amazon-lumberyard-bistro` (`Bistro_v5_2.zip`):** نظراً لكثافة المضلعات الإضاءة المعمارية في مشهد Bistro، قررنا تخصيصها **كاستوديو تصوير سينمائي (`Cinematic Studio`)** لالتقاط خلفيات القائمة الرئيسية وشاشة التحميل ووضع التصوير `Photo Mode (F8)`، مع إمكانية قيادة المركبات وتصويرها داخلها في وضع الاستوديو.
2. **التعامل مع قيد 10MB في Sandbox Drive API:**
   - تم تضمين سكربت `download_heavy_assets.ps1` الذي يقوم بتحميل الأصول التي يتجاوز حجمها 10 ميجابايت من روابط Google Drive مباشرة على جهاز المالك إلى مجلدات `mods/` و `client/assets/`، بينما يتعرف عليها المحرك (`hicar.rs`, `ui.rs`, `mods.rs`) تلقائياً فور وجودها.
