#include "util.h"

namespace fm {

QString normalizeRemoteDir(QString path)
{
    path = path.trimmed();
    path.replace(QLatin1Char('/'), QLatin1Char('\\'));
    if (path.isEmpty()) {
        return QStringLiteral("D:\\");
    }
    if (!path.endsWith(QLatin1Char('\\'))) {
        path += QLatin1Char('\\');
    }
    return path;
}

QString joinRemote(const QString &dir, const QString &name)
{
    QString clean = name;
    while (clean.startsWith(QLatin1Char('\\')) || clean.startsWith(QLatin1Char('/'))) {
        clean.remove(0, 1);
    }
    return normalizeRemoteDir(dir) + clean;
}

QString parentRemote(const QString &dir)
{
    QString path = normalizeRemoteDir(dir);
    if (path.size() <= 3) {
        return path;
    }
    path.chop(1);
    const qsizetype separator = path.lastIndexOf(QLatin1Char('\\'));
    if (separator < 2) {
        return path.left(3);
    }
    return path.left(separator + 1);
}

bool isRemoteRoot(const QString &dir)
{
    return normalizeRemoteDir(dir).size() <= 3;
}

QString driveOf(const QString &path)
{
    const QString normalized = normalizeRemoteDir(path);
    if (normalized.size() < 2 || normalized.at(1) != QLatin1Char(':')) {
        return QString();
    }
    return normalized.left(2).toUpper();
}

QString formatBytes(quint64 bytes)
{
    if (bytes < 1024) {
        return QStringLiteral("%1 B").arg(bytes);
    }
    static const char *const units[] = { "KiB", "MiB", "GiB", "TiB" };
    double amount = static_cast<double>(bytes);
    for (int index = 0; index < 4; ++index) {
        amount /= 1024.0;
        if (amount < 1024.0 || index == 3) {
            return QString::number(amount, 'f', 2) + QLatin1Char(' ')
                + QString::fromLatin1(units[index]);
        }
    }
    return QStringLiteral("%1 B").arg(bytes);
}

} // namespace fm
