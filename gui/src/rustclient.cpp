#include "rustclient.h"

#include "fm_explore.h"

#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMetaObject>

RustClient::RustClient(QObject *parent)
    : QObject(parent)
{
    m_client = fm_client_new(0x06D3, 0x21B0, 5000, 3, 60000, 0);
    if (m_client) {
        fm_client_set_progress_callback(m_client, &RustClient::progressTrampoline, this);
    }
}

RustClient::~RustClient()
{
    if (m_client) {
        fm_client_set_progress_callback(m_client, nullptr, nullptr);
        fm_client_free(m_client);
        m_client = nullptr;
    }
}

void RustClient::setProgressEnabled(bool enabled)
{
    if (!m_client) {
        return;
    }
    fm_client_set_progress_callback(
        m_client, enabled ? &RustClient::progressTrampoline : nullptr,
        enabled ? static_cast<void *>(this) : nullptr);
}

FmReply RustClient::call(const QString &command, const QString &arg1,
                         const QString &arg2)
{
    FmReply reply;
    if (!m_client) {
        reply.error = tr("The Rust core is not available.");
        reply.kind = QStringLiteral("internal");
        return reply;
    }

    const QByteArray commandUtf8 = command.toUtf8();
    const QByteArray arg1Utf8 = arg1.isNull() ? QByteArray() : arg1.toUtf8();
    const QByteArray arg2Utf8 = arg2.isNull() ? QByteArray() : arg2.toUtf8();

    char *raw = fm_call(m_client, commandUtf8.constData(),
                        arg1.isNull() ? nullptr : arg1Utf8.constData(),
                        arg2.isNull() ? nullptr : arg2Utf8.constData());
    if (!raw) {
        reply.error = tr("The Rust core returned an empty response.");
        reply.kind = QStringLiteral("internal");
        return reply;
    }

    const QByteArray payload(raw);
    fm_free_string(raw);

    QJsonParseError parseError {};
    const QJsonDocument document = QJsonDocument::fromJson(payload, &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        reply.error = tr("Invalid response from the Rust core: %1").arg(parseError.errorString());
        reply.kind = QStringLiteral("internal");
        return reply;
    }

    const QJsonObject object = document.object();
    reply.ok = object.value(QStringLiteral("ok")).toBool();
    if (reply.ok) {
        reply.data = object.value(QStringLiteral("data"));
        return reply;
    }

    reply.error = object.value(QStringLiteral("error")).toString();
    if (reply.error.isEmpty()) {
        reply.error = tr("Unknown error.");
    }
    reply.kind = object.value(QStringLiteral("kind")).toString();

    const QJsonValue status = object.value(QStringLiteral("status"));
    if (status.isDouble()) {
        reply.hasStatus = true;
        reply.status = status.toInt();
    }
    const QJsonValue osError = object.value(QStringLiteral("os_error"));
    if (osError.isDouble()) {
        reply.hasOsError = true;
        reply.osError = osError.toInt();
    }
    return reply;
}

void RustClient::progressTrampoline(void *user, uint64_t current, int64_t total)
{
    auto *self = static_cast<RustClient *>(user);
    if (!self) {
        return;
    }
    QMetaObject::invokeMethod(
        self,
        [self, current, total]() {
            emit self->progress(static_cast<quint64>(current), static_cast<qint64>(total));
        },
        Qt::QueuedConnection);
}

QString RustClient::version()
{
    return QString::fromUtf8(fm_version());
}
