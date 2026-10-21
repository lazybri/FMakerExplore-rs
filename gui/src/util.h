#pragma once

#include <QString>

namespace fm {

QString normalizeRemoteDir(QString path);
QString joinRemote(const QString &dir, const QString &name);
QString parentRemote(const QString &dir);
bool isRemoteRoot(const QString &dir);
QString driveOf(const QString &path);
QString formatBytes(quint64 bytes);

} // namespace fm
