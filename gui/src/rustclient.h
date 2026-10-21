#pragma once

#include <QJsonValue>
#include <QObject>
#include <QString>
#include <QtGlobal>

#include <cstdint>

struct FmClient;

struct FmReply {
    bool ok = false;
    QJsonValue data;
    QString error;
    QString kind;
    int status = -1;
    int osError = 0;
    bool hasStatus = false;
    bool hasOsError = false;

    bool isRemoteError() const { return kind == QStringLiteral("remote"); }
};

class RustClient : public QObject
{
    Q_OBJECT

public:
    explicit RustClient(QObject *parent = nullptr);
    ~RustClient() override;

    bool isValid() const { return m_client != nullptr; }
    FmReply call(const QString &command, const QString &arg1 = QString(),
                 const QString &arg2 = QString());
    void setProgressEnabled(bool enabled);
    static QString version();

signals:
    void progress(quint64 current, qint64 total);

private:
    static void progressTrampoline(void *user, uint64_t current, int64_t total);
    FmClient *m_client = nullptr;
};
