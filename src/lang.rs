use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    De,
    Fr,
    Es,
    Tr,
}

impl Default for Lang {
    fn default() -> Self {
        Lang::En
    }
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::En, Lang::De, Lang::Fr, Lang::Es, Lang::Tr];

    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Fr => "Français",
            Lang::Es => "Español",
            Lang::Tr => "Türkçe",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::De => 1,
            Lang::Fr => 2,
            Lang::Es => 3,
            Lang::Tr => 4,
        }
    }

    pub fn from_index(i: usize) -> Lang {
        Lang::ALL.get(i).copied().unwrap_or(Lang::En)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum K {
    Settings,
    ThisMetricSettings,
    ThisMetricView,
    ThisMetricChart,
    ChartAll,
    Floats,
    ViewAll,
    Compact,
    Refresh,
    Autostart,
    Exit,
    CloseWidget,
    TransparentAll,
    TopmostAll,
    Title,
    General,
    Metrics,
    Colors,
    BarLook,
    FloatLook,
    RefreshMs,
    CapacityS,
    Theme,
    Language,
    FontLabel,
    FontValue,
    FloatFont,
    ChartWidth,
    Show,
    View,
    ChartType,
    Floating,
    Widget,
    Background,
    Opacity,
    Border,
    Transparent,
    AlwaysTop,
    HintTransparent,
    BarTransparent,
    Save,
    Close,
    Applied,
    ThemeAuto,
    ThemeDark,
    ThemeLight,
    ViewText,
    ViewGraph,
    ViewBoth,
    GraphNone,
    GraphBar,
    GraphLine,
    GraphPie,
    GraphLed,
    LabelColor,
    WarnColor,
    ResetColors,
    Usage,
    Used,
    Total,
    Read,
    Write,
    Download,
    Upload,
}

fn row(k: K) -> [&'static str; 5] {
    match k {
        K::Settings => [
            "Settings",
            "Einstellungen",
            "Paramètres",
            "Ajustes",
            "Ayarlar",
        ],
        K::ThisMetricSettings => [
            "Settings for this metric...",
            "Einstellungen für diese Metrik...",
            "Paramètres de cette métrique...",
            "Ajustes de esta métrica...",
            "Bu metrik için ayarlar...",
        ],
        K::ThisMetricView => [
            "View for this metric",
            "Ansicht für diese Metrik",
            "Affichage de cette métrique",
            "Vista de esta métrica",
            "Bu metriğin görünümü",
        ],
        K::ThisMetricChart => [
            "Chart for this metric",
            "Diagramm für diese Metrik",
            "Graphique de cette métrique",
            "Gráfico de esta métrica",
            "Bu metriğin grafiği",
        ],
        K::ChartAll => [
            "Chart type (all)",
            "Diagrammtyp (alle)",
            "Type de graphique (tous)",
            "Tipo de gráfico (todos)",
            "Grafik türü (tümü)",
        ],
        K::Floats => [
            "Floating widgets",
            "Schwebende Widgets",
            "Widgets flottants",
            "Widgets flotantes",
            "Yüzen widget'lar",
        ],
        K::ViewAll => [
            "View for all",
            "Ansicht für alle",
            "Affichage pour tous",
            "Vista para todos",
            "Tümü için görünüm",
        ],
        K::Compact => [
            "Compact layout",
            "Kompaktes Layout",
            "Disposition compacte",
            "Diseño compacto",
            "Sıkı yerleşim",
        ],
        K::Refresh => ["Refresh", "Aktualisieren", "Actualiser", "Actualizar", "Yenile"],
        K::Autostart => [
            "Start with Windows",
            "Mit Windows starten",
            "Démarrer avec Windows",
            "Iniciar con Windows",
            "Windows açılışında başlat",
        ],
        K::Exit => ["Exit", "Beenden", "Quitter", "Salir", "Çıkış"],
        K::CloseWidget => [
            "Close this widget",
            "Dieses Widget schließen",
            "Fermer ce widget",
            "Cerrar este widget",
            "Bu widget'ı kapat",
        ],
        K::TransparentAll => [
            "Transparent (all)",
            "Transparent (alle)",
            "Transparent (tous)",
            "Transparente (todos)",
            "Şeffaf (tümü)",
        ],
        K::TopmostAll => [
            "Always on top (all)",
            "Immer im Vordergrund (alle)",
            "Toujours au premier plan (tous)",
            "Siempre visible (todos)",
            "Her zaman üstte (tümü)",
        ],
        K::Title => [
            "StatMon Settings",
            "StatMon Einstellungen",
            "StatMon Paramètres",
            "StatMon Ajustes",
            "StatMon Ayarlar",
        ],
        K::General => ["General", "Allgemein", "Général", "General", "Genel"],
        K::Metrics => ["Metrics", "Metriken", "Métriques", "Métricas", "Metrikler"],
        K::Colors => ["Colors", "Farben", "Couleurs", "Colores", "Renkler"],
        K::BarLook => [
            "Taskbar widget",
            "Taskleisten-Widget",
            "Widget de la barre des tâches",
            "Widget de la barra de tareas",
            "Görev çubuğu widget'ı",
        ],
        K::FloatLook => [
            "Floating widget style",
            "Stil des schwebenden Widgets",
            "Style du widget flottant",
            "Estilo del widget flotante",
            "Yüzen widget stili",
        ],
        K::RefreshMs => [
            "Refresh (ms)",
            "Aktualisierung (ms)",
            "Rafraîchir (ms)",
            "Actualizar (ms)",
            "Yenileme (ms)",
        ],
        K::CapacityS => [
            "Capacity (s)",
            "Kapazität (s)",
            "Capacité (s)",
            "Capacidad (s)",
            "Kapasite (sn)",
        ],
        K::Theme => ["Theme", "Design", "Thème", "Tema", "Tema"],
        K::Language => ["Language", "Sprache", "Langue", "Idioma", "Dil"],
        K::FontLabel => [
            "Label font",
            "Schrift Beschriftung",
            "Police libellé",
            "Fuente etiqueta",
            "Etiket fontu",
        ],
        K::FontValue => [
            "Value font",
            "Schrift Wert",
            "Police valeur",
            "Fuente valor",
            "Değer fontu",
        ],
        K::FloatFont => [
            "Float font",
            "Schrift schwebend",
            "Police flottant",
            "Fuente flotante",
            "Yüzen font",
        ],
        K::ChartWidth => [
            "Chart width",
            "Diagrammbreite",
            "Largeur graphique",
            "Ancho gráfico",
            "Grafik genişlik",
        ],
        K::Show => ["Show", "Anzeigen", "Afficher", "Mostrar", "Göster"],
        K::View => ["View", "Ansicht", "Affichage", "Vista", "Görünüm"],
        K::ChartType => [
            "Chart type",
            "Diagrammtyp",
            "Type de graphique",
            "Tipo de gráfico",
            "Grafik türü",
        ],
        K::Floating => ["Floating", "Schwebend", "Flottant", "Flotante", "Yüzen"],
        K::Widget => ["Widget", "Widget", "Widget", "Widget", "Widget"],
        K::Background => [
            "Background",
            "Hintergrund",
            "Arrière-plan",
            "Fondo",
            "Arka plan",
        ],
        K::Opacity => ["Opacity", "Deckkraft", "Opacité", "Opacidad", "Opaklık"],
        K::Border => ["Border", "Rahmen", "Bordure", "Borde", "Kenarlık"],
        K::Transparent => [
            "Transparent",
            "Transparent",
            "Transparent",
            "Transparente",
            "Şeffaf",
        ],
        K::AlwaysTop => [
            "Always on top",
            "Immer oben",
            "Toujours visible",
            "Siempre visible",
            "Her zaman üstte",
        ],
        K::HintTransparent => [
            "(background hidden when transparent)",
            "(Hintergrund bei Transparenz ausgeblendet)",
            "(fond masqué si transparent)",
            "(fondo oculto si es transparente)",
            "(şeffafken arka plan görünmez)",
        ],
        K::BarTransparent => [
            "Transparent bar",
            "Transparente Leiste",
            "Barre transparente",
            "Barra transparente",
            "Şeffaf çubuk",
        ],
        K::Save => ["Save", "Speichern", "Enregistrer", "Guardar", "Kaydet"],
        K::Close => ["Close", "Schließen", "Fermer", "Cerrar", "Kapat"],
        K::Applied => ["Applied", "Angewendet", "Appliqué", "Aplicado", "Uygulandı"],
        K::ThemeAuto => ["Auto", "Automatisch", "Auto", "Auto", "Otomatik"],
        K::ThemeDark => ["Dark", "Dunkel", "Sombre", "Oscuro", "Koyu"],
        K::ThemeLight => ["Light", "Hell", "Clair", "Claro", "Açık"],
        K::ViewText => ["Text", "Text", "Texte", "Texto", "Metin"],
        K::ViewGraph => ["Graph", "Diagramm", "Graphique", "Gráfico", "Grafik"],
        K::ViewBoth => [
            "Text+Graph",
            "Text+Diagramm",
            "Texte+Graphique",
            "Texto+Gráfico",
            "Metin+Grafik",
        ],
        K::GraphNone => ["None", "Keine", "Aucun", "Ninguno", "Yok"],
        K::GraphBar => ["Bar", "Balken", "Barres", "Barras", "Bar"],
        K::GraphLine => ["Line", "Linie", "Ligne", "Línea", "Çizgi"],
        K::GraphPie => ["Pie", "Kreis", "Secteurs", "Circular", "Pasta"],
        K::LabelColor => [
            "Label color",
            "Beschriftungsfarbe",
            "Couleur libellé",
            "Color etiqueta",
            "Etiket rengi",
        ],
        K::WarnColor => [
            "Warning color",
            "Warnfarbe",
            "Couleur d'alerte",
            "Color de alerta",
            "Uyarı rengi",
        ],
        K::ResetColors => [
            "Reset colors",
            "Farben zurücksetzen",
            "Réinitialiser les couleurs",
            "Restablecer colores",
            "Renkleri sıfırla",
        ],
        K::GraphLed => ["LED", "LED", "LED", "LED", "LED"],
        K::Usage => ["Usage", "Auslastung", "Utilisation", "Uso", "Kullanım"],
        K::Used => ["Used", "Belegt", "Utilisé", "Usado", "Kullanılan"],
        K::Total => ["Total", "Gesamt", "Total", "Total", "Toplam"],
        K::Read => ["Read", "Lesen", "Lecture", "Lectura", "Okuma"],
        K::Write => ["Write", "Schreiben", "Écriture", "Escritura", "Yazma"],
        K::Download => [
            "Download",
            "Download",
            "Téléchargement",
            "Descarga",
            "İndirme",
        ],
        K::Upload => ["Upload", "Upload", "Envoi", "Subida", "Yükleme"],
    }
}

pub fn t(lang: Lang, k: K) -> &'static str {
    row(k)[lang.index()]
}
