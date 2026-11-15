# إصلاح خطأ الصوت (UnsupportedFormat)
- السبب: مكتبة quad-snd تدعم OGG(Vorbis) و WAV فقط، والكود كان يحمّل main_menu_theme.mp3 أولاً فيحدث panic داخل خيط الصوت.
- الإصلاح: تحويل الموسيقى إلى main_menu_theme.ogg، وتعديل client/src/ui.rs ليتحقق من صيغة الملف (OggS/RIFF) قبل تحميله، وإزالة mp3 من سكربتات التحميل.
