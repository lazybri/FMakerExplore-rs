#include "theme.h"

#include <QApplication>
#include <QColor>
#include <QGuiApplication>
#include <QPalette>
#include <QSettings>
#include <QStyle>
#include <QStyleFactory>
#include <QStyleHints>

namespace {

QString g_originalStyle;
bool g_dark = false;

QString originalStyleName()
{
    const QStyle *style = QApplication::style();
    const QString name = style ? style->objectName() : QString();
    return name.isEmpty() ? QStringLiteral("Fusion") : name;
}

QStyle *createStyle(const QString &name)
{
    const QStringList keys = QStyleFactory::keys();
    for (const QString &key : keys) {
        if (key.compare(name, Qt::CaseInsensitive) == 0) {
            if (QStyle *style = QStyleFactory::create(key)) {
                return style;
            }
        }
    }
    return QStyleFactory::create(QStringLiteral("Fusion"));
}

QPalette darkPalette()
{
    const QColor window(53, 53, 53);
    const QColor base(35, 35, 35);
    const QColor text(220, 220, 220);
    const QColor disabled(127, 127, 127);

    QPalette palette;
    palette.setColor(QPalette::Window, window);
    palette.setColor(QPalette::WindowText, text);
    palette.setColor(QPalette::Base, base);
    palette.setColor(QPalette::AlternateBase, QColor(66, 66, 66));
    palette.setColor(QPalette::ToolTipBase, window);
    palette.setColor(QPalette::ToolTipText, text);
    palette.setColor(QPalette::PlaceholderText, QColor(150, 150, 150));
    palette.setColor(QPalette::Text, text);
    palette.setColor(QPalette::Button, window);
    palette.setColor(QPalette::ButtonText, text);
    palette.setColor(QPalette::BrightText, QColor(255, 90, 90));
    palette.setColor(QPalette::Link, QColor(64, 158, 255));
    palette.setColor(QPalette::Highlight, QColor(42, 130, 218));
    palette.setColor(QPalette::HighlightedText, Qt::white);
    palette.setColor(QPalette::Light, QColor(80, 80, 80));
    palette.setColor(QPalette::Midlight, QColor(66, 66, 66));
    palette.setColor(QPalette::Mid, QColor(45, 45, 45));
    palette.setColor(QPalette::Dark, QColor(28, 28, 28));
    palette.setColor(QPalette::Shadow, QColor(18, 18, 18));

    palette.setColor(QPalette::Disabled, QPalette::Text, disabled);
    palette.setColor(QPalette::Disabled, QPalette::WindowText, disabled);
    palette.setColor(QPalette::Disabled, QPalette::ButtonText, disabled);
    palette.setColor(QPalette::Disabled, QPalette::HighlightedText, disabled);
    palette.setColor(QPalette::Disabled, QPalette::Base, QColor(45, 45, 45));
    palette.setColor(QPalette::Disabled, QPalette::Button, QColor(45, 45, 45));
    return palette;
}

} // namespace

namespace Theme {

bool resolveDarkMode()
{
    QSettings settings;
    const QString stored = settings.value(QStringLiteral("appearance/theme")).toString();
    if (stored == QStringLiteral("dark")) {
        return true;
    }
    if (stored == QStringLiteral("light")) {
        return false;
    }

#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
    const Qt::ColorScheme scheme = QGuiApplication::styleHints()->colorScheme();
    if (scheme == Qt::ColorScheme::Dark) {
        return true;
    }
    if (scheme == Qt::ColorScheme::Light) {
        return false;
    }
#endif

    return QApplication::palette().color(QPalette::Window).lightness() < 128;
}

void apply(bool dark)
{
    if (g_originalStyle.isEmpty()) {
        g_originalStyle = originalStyleName();
    }
    g_dark = dark;

#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
    QGuiApplication::styleHints()->setColorScheme(
        dark ? Qt::ColorScheme::Dark : Qt::ColorScheme::Light);
#endif

    if (dark) {
        // Fusion is the only built-in style that consistently honours a custom
        // palette on Windows, macOS, and Wayland.
        if (QStyle *style = createStyle(QStringLiteral("Fusion"))) {
            QApplication::setStyle(style);
        }
        QApplication::setPalette(darkPalette());
    } else {
        if (QStyle *style = createStyle(g_originalStyle)) {
            QApplication::setStyle(style);
        }
        QApplication::setPalette(QApplication::style()->standardPalette());
    }
}

void initialize()
{
    apply(resolveDarkMode());
}

bool isDark()
{
    return g_dark;
}

} // namespace Theme
