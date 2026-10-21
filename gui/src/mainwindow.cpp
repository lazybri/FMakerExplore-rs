#include "mainwindow.h"

#include "theme.h"
#include "util.h"

#include <QAction>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QFutureWatcher>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QInputDialog>
#include <QJsonArray>
#include <QJsonObject>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMessageBox>
#include <QModelIndex>
#include <QProgressBar>
#include <QPushButton>
#include <QSettings>
#include <QSignalBlocker>
#include <QStandardPaths>
#include <QStatusBar>
#include <QTableWidget>
#include <QTimer>
#include <QToolBar>
#include <QVBoxLayout>
#include <QtConcurrent/QtConcurrentRun>

MainWindow::MainWindow(bool autoConnect, QWidget *parent)
    : QMainWindow(parent)
{
    m_pool.setMaxThreadCount(1);
    m_rust = new RustClient(this);

    buildUi();
    loadSettings();

    connect(m_rust, &RustClient::progress, this, &MainWindow::handleProgress);
    updateActions();

    if (autoConnect) {
        QTimer::singleShot(0, this, &MainWindow::connectDevice);
    }
}

MainWindow::~MainWindow()
{
    saveSettings();
    m_pool.clear();
    m_pool.waitForDone();
    if (m_rust) {
        m_rust->setProgressEnabled(false);
    }
}

void MainWindow::buildUi()
{
    setWindowTitle(tr("FMakerExplore"));
    resize(920, 620);

    auto addAction = [this](const QString &text, const QKeySequence &shortcut) {
        auto *action = new QAction(text, this);
        if (!shortcut.isEmpty()) {
            action->setShortcut(shortcut);
        }
        return action;
    };

    m_connectAction = addAction(tr("Connect"), QKeySequence());
    m_upAction = addAction(tr("Up"), QKeySequence(QStringLiteral("Alt+Up")));
    m_refreshAction = addAction(tr("Refresh"), QKeySequence(Qt::Key_F5));
    m_mkdirAction = addAction(tr("New Folder"), QKeySequence());
    m_uploadAction = addAction(tr("Upload"), QKeySequence());
    m_downloadAction = addAction(tr("Download"), QKeySequence());
    m_deleteAction = addAction(tr("Delete"), QKeySequence(Qt::Key_Delete));
    m_openAction = addAction(tr("Open"), QKeySequence());
    m_readonlyAction = addAction(tr("Read-only"), QKeySequence());
    m_themeAction = addAction(tr("Dark mode"), QKeySequence(QStringLiteral("Ctrl+Shift+D")));

    m_toolbar = addToolBar(tr("Main"));
    m_toolbar->setMovable(false);
    m_toolbar->setToolButtonStyle(Qt::ToolButtonTextOnly);
    m_toolbar->addAction(m_connectAction);
    m_toolbar->addSeparator();
    m_toolbar->addAction(m_upAction);
    m_toolbar->addAction(m_refreshAction);
    m_toolbar->addSeparator();
    m_toolbar->addAction(m_mkdirAction);
    m_toolbar->addAction(m_uploadAction);
    m_toolbar->addAction(m_downloadAction);
    m_toolbar->addAction(m_deleteAction);
    m_toolbar->addAction(m_readonlyAction);
    m_toolbar->addSeparator();
    m_toolbar->addWidget(new QLabel(tr("Drive:"), this));
    m_driveCombo = new QComboBox(this);
    m_driveCombo->setMinimumWidth(72);
    m_toolbar->addWidget(m_driveCombo);
    m_toolbar->addSeparator();
    m_toolbar->addAction(m_themeAction);

    auto *central = new QWidget(this);
    auto *layout = new QVBoxLayout(central);
    layout->setContentsMargins(8, 8, 8, 8);
    layout->setSpacing(6);

    auto *pathRow = new QHBoxLayout();
    pathRow->addWidget(new QLabel(tr("Path:"), central));
    m_pathEdit = new QLineEdit(central);
    m_pathEdit->setText(m_path);
    m_pathEdit->setPlaceholderText(QStringLiteral("D:\\WcdmaMp\\"));
    m_pathEdit->setClearButtonEnabled(true);
    pathRow->addWidget(m_pathEdit, 1);
    m_goButton = new QPushButton(tr("Go"), central);
    pathRow->addWidget(m_goButton);
    layout->addLayout(pathRow);

    m_table = new QTableWidget(0, 4, central);
    m_table->setHorizontalHeaderLabels({ tr("Name"), tr("Size"), tr("Attributes"), tr("Year") });
    m_table->horizontalHeader()->setSectionResizeMode(0, QHeaderView::Stretch);
    for (int column = 1; column < 4; ++column) {
        m_table->horizontalHeader()->setSectionResizeMode(column, QHeaderView::ResizeToContents);
    }
    m_table->verticalHeader()->setVisible(false);
    m_table->setSelectionBehavior(QAbstractItemView::SelectRows);
    m_table->setSelectionMode(QAbstractItemView::SingleSelection);
    m_table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    m_table->setAlternatingRowColors(true);
    m_table->setShowGrid(false);
    m_table->setWordWrap(false);
    m_table->setTextElideMode(Qt::ElideMiddle);
    m_table->setSortingEnabled(true);
    m_table->setContextMenuPolicy(Qt::CustomContextMenu);
    layout->addWidget(m_table, 1);
    setCentralWidget(central);

    m_status = new QLabel(tr("Not connected"), this);
    statusBar()->addWidget(m_status, 1);
    m_progress = new QProgressBar(this);
    m_progress->setMaximumWidth(240);
    m_progress->setTextVisible(true);
    m_progress->hide();
    statusBar()->addPermanentWidget(m_progress);
    m_space = new QLabel(tr("D: free space unknown"), this);
    statusBar()->addPermanentWidget(m_space);

    connect(m_connectAction, &QAction::triggered, this, &MainWindow::connectDevice);
    connect(m_upAction, &QAction::triggered, this, &MainWindow::goUp);
    connect(m_refreshAction, &QAction::triggered, this, &MainWindow::refresh);
    connect(m_mkdirAction, &QAction::triggered, this, &MainWindow::makeDirectory);
    connect(m_uploadAction, &QAction::triggered, this, &MainWindow::uploadFile);
    connect(m_downloadAction, &QAction::triggered, this, &MainWindow::downloadSelected);
    connect(m_deleteAction, &QAction::triggered, this, &MainWindow::deleteSelected);
    connect(m_openAction, &QAction::triggered, this, &MainWindow::openSelected);
    connect(m_readonlyAction, &QAction::triggered, this, &MainWindow::toggleReadonly);
    connect(m_themeAction, &QAction::triggered, this, [this]() {
        const bool dark = !Theme::isDark();
        m_themeAction->setText(dark ? tr("Light mode") : tr("Dark mode"));
        Theme::apply(dark);
        QSettings settings;
        settings.setValue(QStringLiteral("appearance/theme"),
                          dark ? QStringLiteral("dark") : QStringLiteral("light"));
        m_status->setText(tr("Theme: %1").arg(dark ? tr("dark") : tr("light")));
    });

    connect(m_pathEdit, &QLineEdit::returnPressed, this,
            [this]() { navigateTo(m_pathEdit->text()); });
    connect(m_goButton, &QPushButton::clicked, this,
            [this]() { navigateTo(m_pathEdit->text()); });
    connect(m_driveCombo, &QComboBox::activated, this, [this](int) {
        const QString drive = m_driveCombo->currentText();
        if (!drive.isEmpty()) {
            navigateTo(drive + QLatin1Char('\\'));
        }
    });
    connect(m_table, &QTableWidget::itemSelectionChanged, this, &MainWindow::updateActions);
    connect(m_table, &QTableWidget::cellDoubleClicked, this,
            [this](int row, int) { openEntry(row); });
    connect(m_table, &QWidget::customContextMenuRequested, this,
            &MainWindow::showContextMenu);
}

void MainWindow::loadSettings()
{
    QSettings settings;
    m_lastLocalDir = settings.value(QStringLiteral("paths/local"), defaultLocalDir()).toString();
    m_path = fm::normalizeRemoteDir(
        settings.value(QStringLiteral("paths/remote"), QStringLiteral("D:\\")).toString());
    m_pathEdit->setText(m_path);

    const QByteArray geometry = settings.value(QStringLiteral("ui/geometry")).toByteArray();
    if (!geometry.isEmpty()) {
        restoreGeometry(geometry);
    }

    m_themeAction->setText(Theme::isDark() ? tr("Light mode") : tr("Dark mode"));
}

void MainWindow::saveSettings()
{
    QSettings settings;
    settings.setValue(QStringLiteral("ui/geometry"), saveGeometry());
    settings.setValue(QStringLiteral("paths/local"), m_lastLocalDir);
    settings.setValue(QStringLiteral("paths/remote"), m_path);
}

void MainWindow::connectDevice()
{
    runTask(tr("Connecting to the phone..."), QStringLiteral("connect"), QString(), QString(),
            [this](const FmReply &reply) {
                if (!reply.ok) {
                    m_connected = false;
                    m_status->setText(tr("Not connected"));
                    updateActions();
                    reportError(tr("Connection failed"), reply);
                    return;
                }
                m_connected = true;
                m_status->setText(tr("Connected"));
                updateActions();
                refreshDrives();
            });
}

void MainWindow::refreshDrives()
{
    runTask(tr("Listing drives..."), QStringLiteral("list_partitions"), QString(), QString(),
            [this](const FmReply &reply) {
                if (!reply.ok) {
                    reportError(tr("Drive list failed"), reply);
                    navigateTo(m_path);
                    return;
                }
                const QJsonArray drives = reply.data.toArray();
                {
                    QSignalBlocker blocker(m_driveCombo);
                    m_driveCombo->clear();
                    for (const QJsonValue &value : drives) {
                        const QString drive = value.toString();
                        if (!drive.isEmpty()) {
                            m_driveCombo->addItem(drive);
                        }
                    }
                    const int index = m_driveCombo->findText(fm::driveOf(m_path));
                    if (index >= 0) {
                        m_driveCombo->setCurrentIndex(index);
                    }
                }
                navigateTo(m_path);
            });
}

void MainWindow::navigateTo(const QString &path)
{
    if (m_busy) {
        return;
    }
    const QString normalized = fm::normalizeRemoteDir(path);
    if (!m_connected) {
        setPath(normalized);
        updateActions();
        return;
    }

    runTask(tr("Opening %1...").arg(normalized), QStringLiteral("list_entries"), normalized,
            QString(), [this, normalized](const FmReply &reply) {
                if (!reply.ok) {
                    m_pathEdit->setText(m_path);
                    reportError(tr("Cannot open folder"), reply);
                    return;
                }
                setPath(normalized);
                populateEntries(reply);
                updateActions();
            });
}

void MainWindow::refresh()
{
    if (!m_connected || m_busy) {
        return;
    }
    const QString path = m_path;
    runTask(tr("Loading %1...").arg(path), QStringLiteral("list_entries"), path, QString(),
            [this, path](const FmReply &reply) {
                if (!reply.ok) {
                    reportError(tr("Directory listing failed"), reply);
                    return;
                }
                setPath(path);
                populateEntries(reply);
                updateActions();
            });
}

void MainWindow::goUp()
{
    if (fm::isRemoteRoot(m_path)) {
        return;
    }
    navigateTo(fm::parentRemote(m_path));
}

void MainWindow::setPath(const QString &path)
{
    m_path = fm::normalizeRemoteDir(path);
    m_pathEdit->setText(m_path);

    QSettings settings;
    settings.setValue(QStringLiteral("paths/remote"), m_path);

    const int index = m_driveCombo->findText(fm::driveOf(m_path));
    if (index >= 0 && index != m_driveCombo->currentIndex()) {
        QSignalBlocker blocker(m_driveCombo);
        m_driveCombo->setCurrentIndex(index);
    }
}

void MainWindow::openEntry(int row)
{
    if (row < 0) {
        return;
    }
    const RemoteEntry entry = entryAt(row);
    if (entry.path.isEmpty()) {
        return;
    }
    if (entry.isDir) {
        navigateTo(entry.path);
        return;
    }
    // The protocol does not always distinguish files from folders in a
    // listing, so probe the entry before treating it as a download.
    tryOpenFolder(entry);
}

void MainWindow::openSelected()
{
    const RemoteEntry entry = currentEntry();
    if (entry.path.isEmpty()) {
        return;
    }
    if (entry.isDir) {
        navigateTo(entry.path);
    } else {
        tryOpenFolder(entry);
    }
}

void MainWindow::tryOpenFolder(const RemoteEntry &entry)
{
    if (m_busy) {
        return;
    }
    runTask(tr("Opening %1...").arg(entry.name), QStringLiteral("list_entries"), entry.path,
            QString(), [this, entry](const FmReply &reply) {
                if (reply.ok) {
                    setPath(entry.path);
                    populateEntries(reply);
                    updateActions();
                    return;
                }
                if (reply.isRemoteError()) {
                    downloadEntry(entry);
                    return;
                }
                reportError(tr("Open failed"), reply);
            });
}

void MainWindow::uploadFile()
{
    const QString local = QFileDialog::getOpenFileName(
        this, tr("Upload File"), m_lastLocalDir, QString(), nullptr,
        QFileDialog::DontUseNativeDialog);
    if (local.isEmpty()) {
        return;
    }
    saveLocalDir(QFileInfo(local).absolutePath());

    bool accepted = false;
    const QString suggested = QFileInfo(local).fileName();
    const QString name = QInputDialog::getText(this, tr("Upload"), tr("Remote file name:"),
                                                QLineEdit::Normal, suggested, &accepted)
                             .trimmed();
    if (!accepted || name.isEmpty()) {
        return;
    }
    if (name.contains(QLatin1Char('\\')) || name.contains(QLatin1Char('/'))
        || name.contains(QLatin1Char(':'))) {
        QMessageBox::warning(this, tr("Upload"),
                             tr("The remote file name must not contain path separators."));
        return;
    }

    const QString remote = fm::joinRemote(m_path, name);
    runTask(tr("Uploading %1...").arg(name), QStringLiteral("push"), local, remote,
            [this, remote](const FmReply &reply) {
                if (!reply.ok) {
                    reportError(tr("Upload failed"), reply);
                    return;
                }
                const qint64 bytes = static_cast<qint64>(
                    reply.data.toObject().value(QStringLiteral("bytes")).toDouble());
                m_status->setText(tr("Uploaded %1 to %2")
                                      .arg(fm::formatBytes(static_cast<quint64>(bytes)), remote));
                refresh();
            },
            true);
}

void MainWindow::downloadSelected()
{
    const RemoteEntry entry = currentEntry();
    if (entry.path.isEmpty() || entry.isDir) {
        return;
    }
    downloadEntry(entry);
}

void MainWindow::downloadEntry(const RemoteEntry &entry)
{
    if (entry.isDir || m_busy) {
        return;
    }
    const QString suggested = QDir(m_lastLocalDir.isEmpty() ? defaultLocalDir() : m_lastLocalDir)
                                  .filePath(entry.name.isEmpty() ? QStringLiteral("file.bin")
                                                                 : entry.name);
    const QString local = QFileDialog::getSaveFileName(
        this, tr("Save File"), suggested, QString(), nullptr,
        QFileDialog::DontUseNativeDialog);
    if (local.isEmpty()) {
        return;
    }
    saveLocalDir(QFileInfo(local).absolutePath());

    const quint64 knownTotal = entry.size > 0 ? static_cast<quint64>(entry.size) : 0;
    runTask(tr("Downloading %1...").arg(entry.name), QStringLiteral("pull"), entry.path, local,
            [this, local](const FmReply &reply) {
                if (!reply.ok) {
                    reportError(tr("Download failed"), reply);
                    return;
                }
                const qint64 bytes = static_cast<qint64>(
                    reply.data.toObject().value(QStringLiteral("bytes")).toDouble());
                m_status->setText(tr("Downloaded %1 to %2")
                                      .arg(fm::formatBytes(static_cast<quint64>(bytes)), local));
            },
            true, knownTotal);
}

void MainWindow::deleteSelected()
{
    const RemoteEntry entry = currentEntry();
    if (entry.path.isEmpty()) {
        return;
    }
    deleteEntry(entry);
}

void MainWindow::deleteEntry(const RemoteEntry &entry)
{
    const QMessageBox::StandardButton answer = QMessageBox::question(
        this, tr("Delete"), tr("Delete %1 from the phone?").arg(entry.path),
        QMessageBox::Yes | QMessageBox::No, QMessageBox::No);
    if (answer != QMessageBox::Yes) {
        return;
    }

    runTask(tr("Deleting %1...").arg(entry.name), QStringLiteral("delete"), entry.path,
            QString(), [this, entry](const FmReply &reply) {
                if (!reply.ok) {
                    reportError(tr("Delete failed"), reply);
                    return;
                }
                m_status->setText(tr("Deleted %1").arg(entry.path));
                refresh();
            });
}

void MainWindow::makeDirectory()
{
    bool accepted = false;
    const QString name = QInputDialog::getText(this, tr("New Folder"), tr("Folder name:"),
                                                QLineEdit::Normal, QString(), &accepted)
                             .trimmed();
    if (!accepted || name.isEmpty()) {
        return;
    }
    if (name.contains(QLatin1Char('\\')) || name.contains(QLatin1Char('/'))
        || name.contains(QLatin1Char(':'))) {
        QMessageBox::warning(this, tr("New Folder"),
                             tr("The folder name must not contain path separators."));
        return;
    }

    const QString remote = fm::joinRemote(m_path, name);
    runTask(tr("Creating %1...").arg(name), QStringLiteral("mkdir"), remote, QString(),
            [this, remote](const FmReply &reply) {
                if (!reply.ok) {
                    reportError(tr("Create folder failed"), reply);
                    return;
                }
                m_status->setText(tr("Created %1").arg(remote));
                refresh();
            });
}

void MainWindow::toggleReadonly()
{
    const RemoteEntry entry = currentEntry();
    if (entry.path.isEmpty() || entry.isDir) {
        return;
    }
    toggleEntryReadonly(entry);
}

void MainWindow::toggleEntryReadonly(const RemoteEntry &entry)
{
    const bool enable = !entry.attributes.contains(QStringLiteral("readonly"));
    runTask(tr("%1 read-only attribute...")
                .arg(enable ? tr("Enabling") : tr("Disabling")),
            QStringLiteral("set_readonly"), entry.path,
            enable ? QStringLiteral("on") : QStringLiteral("off"),
            [this, enable](const FmReply &reply) {
                if (!reply.ok) {
                    reportError(tr("Read-only change failed"), reply);
                    return;
                }
                m_status->setText(enable ? tr("Read-only enabled") : tr("Read-only disabled"));
                refresh();
            });
}

void MainWindow::showContextMenu(const QPoint &pos)
{
    if (currentRow() < 0) {
        return;
    }
    QMenu menu(this);
    menu.addAction(m_openAction);
    menu.addAction(m_downloadAction);
    menu.addSeparator();
    menu.addAction(m_readonlyAction);
    menu.addAction(m_deleteAction);
    menu.exec(m_table->viewport()->mapToGlobal(pos));
}

void MainWindow::updateActions()
{
    const bool idle = !m_busy;
    const int row = currentRow();
    const bool hasSelection = row >= 0;
    const RemoteEntry entry = hasSelection ? entryAt(row) : RemoteEntry();

    m_connectAction->setText(m_connected ? tr("Reconnect") : tr("Connect"));
    m_connectAction->setEnabled(idle);
    m_upAction->setEnabled(idle && m_connected && !fm::isRemoteRoot(m_path));
    m_refreshAction->setEnabled(idle && m_connected);
    m_mkdirAction->setEnabled(idle && m_connected);
    m_uploadAction->setEnabled(idle && m_connected);

    m_openAction->setEnabled(idle && m_connected && hasSelection);
    m_downloadAction->setEnabled(idle && m_connected && hasSelection && !entry.isDir);
    m_deleteAction->setEnabled(idle && m_connected && hasSelection);
    m_readonlyAction->setEnabled(idle && m_connected && hasSelection && !entry.isDir);

    m_pathEdit->setEnabled(idle && m_connected);
    m_goButton->setEnabled(idle && m_connected);
    m_driveCombo->setEnabled(idle && m_connected);
}

void MainWindow::setBusy(bool busy, const QString &message, bool showProgress,
                         quint64 knownTotal)
{
    m_busy = busy;
    if (busy) {
        if (!message.isEmpty()) {
            m_status->setText(message);
        }
        if (showProgress) {
            m_progressTotal = knownTotal;
            m_progress->setVisible(true);
            if (knownTotal > 0) {
                m_progress->setRange(0, 1000);
                m_progress->setValue(0);
                m_progress->setFormat(tr("0 / %1").arg(fm::formatBytes(knownTotal)));
            } else {
                m_progress->setRange(0, 0);
                m_progress->setFormat(QString());
            }
        } else {
            m_progress->hide();
        }
    } else {
        m_progressTotal = 0;
        m_progress->hide();
        m_progress->setRange(0, 100);
        m_progress->reset();
    }
    updateActions();
}

void MainWindow::runTask(const QString &description, const QString &command,
                         const QString &arg1, const QString &arg2,
                         std::function<void(const FmReply &)> done, bool showProgress,
                         quint64 knownTotal)
{
    if (m_busy) {
        return;
    }
    setBusy(true, description, showProgress, knownTotal);

    const QString commandCopy = command;
    const QString arg1Copy = arg1;
    const QString arg2Copy = arg2;

    auto *watcher = new QFutureWatcher<FmReply>(this);
    connect(watcher, &QFutureWatcher<FmReply>::finished, this,
            [this, watcher, done = std::move(done)]() mutable {
                const FmReply reply = watcher->result();
                watcher->deleteLater();
                setBusy(false, QString(), false);
                done(reply);
            });
    watcher->setFuture(QtConcurrent::run(&m_pool, [this, commandCopy, arg1Copy, arg2Copy]() {
        return m_rust->call(commandCopy, arg1Copy, arg2Copy);
    }));
}

void MainWindow::populateEntries(const FmReply &reply)
{
    const QJsonObject root = reply.data.toObject();
    const QJsonArray entries = root.value(QStringLiteral("entries")).toArray();

    m_table->setSortingEnabled(false);
    m_table->clearContents();
    m_table->setRowCount(entries.size());

    for (int row = 0; row < entries.size(); ++row) {
        const QJsonObject object = entries.at(row).toObject();
        const QString name = object.value(QStringLiteral("name")).toString();
        const bool isDir = object.value(QStringLiteral("is_dir")).toBool();
        const QString path = object.value(QStringLiteral("path")).toString();
        const qint64 size = object.value(QStringLiteral("size")).isDouble()
            ? static_cast<qint64>(object.value(QStringLiteral("size")).toDouble())
            : -1;
        const int year = object.value(QStringLiteral("year")).toInt();

        QStringList attributes;
        const QJsonArray attributeArray = object.value(QStringLiteral("attributes")).toArray();
        for (const QJsonValue &value : attributeArray) {
            attributes << value.toString();
        }

        auto *nameItem = new QTableWidgetItem(name);
        nameItem->setData(Qt::UserRole, path);
        nameItem->setData(Qt::UserRole + 1, isDir);
        nameItem->setData(Qt::UserRole + 2, static_cast<qlonglong>(size));
        nameItem->setData(Qt::UserRole + 3, attributes.join(QLatin1Char(',')));
        nameItem->setToolTip(path);
        m_table->setItem(row, 0, nameItem);

        auto *sizeItem = new QTableWidgetItem(
            isDir ? tr("<DIR>")
                  : (size >= 0 ? fm::formatBytes(static_cast<quint64>(size)) : QString()));
        sizeItem->setTextAlignment(Qt::AlignRight | Qt::AlignVCenter);
        m_table->setItem(row, 1, sizeItem);

        m_table->setItem(row, 2, new QTableWidgetItem(attributes.join(QStringLiteral(", "))));
        m_table->setItem(row, 3,
                         new QTableWidgetItem(year > 0 ? QString::number(year) : QString()));
    }

    m_table->setSortingEnabled(true);
    m_table->sortItems(0, Qt::AscendingOrder);
    m_table->clearSelection();

    const QJsonValue available = root.value(QStringLiteral("available"));
    if (available.isDouble()) {
        m_space->setText(tr("D: free %1")
                             .arg(fm::formatBytes(static_cast<quint64>(available.toDouble()))));
    } else {
        m_space->setText(tr("D: free space unknown"));
    }
    m_status->setText(tr("%1 item(s)").arg(entries.size()));
}

RemoteEntry MainWindow::entryAt(int row) const
{
    RemoteEntry entry;
    const QTableWidgetItem *item = m_table->item(row, 0);
    if (!item) {
        return entry;
    }
    entry.name = item->text();
    entry.path = item->data(Qt::UserRole).toString();
    entry.isDir = item->data(Qt::UserRole + 1).toBool();
    entry.size = item->data(Qt::UserRole + 2).toLongLong();
    const QString attributes = item->data(Qt::UserRole + 3).toString();
    if (!attributes.isEmpty()) {
        entry.attributes = attributes.split(QLatin1Char(','), Qt::SkipEmptyParts);
    }
    return entry;
}

RemoteEntry MainWindow::currentEntry() const
{
    return entryAt(currentRow());
}

int MainWindow::currentRow() const
{
    if (!m_table->selectionModel()) {
        return -1;
    }
    const QModelIndexList rows = m_table->selectionModel()->selectedRows();
    return rows.isEmpty() ? -1 : rows.first().row();
}

void MainWindow::reportError(const QString &title, const FmReply &reply)
{
    const QString message = reply.error.isEmpty() ? tr("Unknown error.") : reply.error;
    m_status->setText(message);
    QMessageBox::warning(this, title, message);
}

void MainWindow::handleProgress(quint64 current, qint64 total)
{
    if (!m_progress->isVisible()) {
        return;
    }
    if (total > 0) {
        m_progressTotal = static_cast<quint64>(total);
    }
    if (m_progressTotal > 0) {
        const quint64 value = (current * 1000) / m_progressTotal;
        m_progress->setRange(0, 1000);
        m_progress->setValue(static_cast<int>(qMin<quint64>(1000, value)));
        m_progress->setFormat(tr("%1 / %2")
                                  .arg(fm::formatBytes(current),
                                       fm::formatBytes(m_progressTotal)));
    } else {
        m_progress->setRange(0, 0);
        m_progress->setFormat(tr("%1 received").arg(fm::formatBytes(current)));
    }
}

void MainWindow::saveLocalDir(const QString &path)
{
    if (path.isEmpty()) {
        return;
    }
    m_lastLocalDir = path;
    QSettings settings;
    settings.setValue(QStringLiteral("paths/local"), path);
}

QString MainWindow::defaultLocalDir()
{
    const QString documents = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    return documents.isEmpty() ? QDir::homePath() : documents;
}
