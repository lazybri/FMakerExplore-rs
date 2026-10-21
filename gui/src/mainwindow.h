#pragma once

#include <QMainWindow>
#include <QPoint>
#include <QStringList>
#include <QThreadPool>

#include <functional>

#include "rustclient.h"

class QAction;
class QComboBox;
class QLabel;
class QLineEdit;
class QProgressBar;
class QPushButton;
class QTableWidget;
class QToolBar;

struct RemoteEntry {
    QString name;
    QString path;
    bool isDir = false;
    qint64 size = -1;
    QStringList attributes;
};

class MainWindow : public QMainWindow
{
    Q_OBJECT

public:
    explicit MainWindow(bool autoConnect = true, QWidget *parent = nullptr);
    ~MainWindow() override;

private:
    void buildUi();
    void loadSettings();
    void saveSettings();

    void connectDevice();
    void refreshDrives();
    void navigateTo(const QString &path);
    void refresh();
    void goUp();
    void setPath(const QString &path);

    void openEntry(int row);
    void openSelected();
    void tryOpenFolder(const RemoteEntry &entry);
    void uploadFile();
    void downloadSelected();
    void downloadEntry(const RemoteEntry &entry);
    void deleteSelected();
    void deleteEntry(const RemoteEntry &entry);
    void makeDirectory();
    void toggleReadonly();
    void toggleEntryReadonly(const RemoteEntry &entry);
    void showContextMenu(const QPoint &pos);

    void updateActions();
    void setBusy(bool busy, const QString &message, bool showProgress,
                 quint64 knownTotal = 0);
    void runTask(const QString &description, const QString &command,
                 const QString &arg1, const QString &arg2,
                 std::function<void(const FmReply &)> done,
                 bool showProgress = false, quint64 knownTotal = 0);
    void populateEntries(const FmReply &reply);
    RemoteEntry entryAt(int row) const;
    RemoteEntry currentEntry() const;
    int currentRow() const;
    void reportError(const QString &title, const FmReply &reply);
    void handleProgress(quint64 current, qint64 total);
    void saveLocalDir(const QString &path);
    static QString defaultLocalDir();

    RustClient *m_rust = nullptr;
    QThreadPool m_pool;

    QToolBar *m_toolbar = nullptr;
    QLineEdit *m_pathEdit = nullptr;
    QPushButton *m_goButton = nullptr;
    QComboBox *m_driveCombo = nullptr;
    QTableWidget *m_table = nullptr;
    QLabel *m_status = nullptr;
    QLabel *m_space = nullptr;
    QProgressBar *m_progress = nullptr;

    QAction *m_connectAction = nullptr;
    QAction *m_upAction = nullptr;
    QAction *m_refreshAction = nullptr;
    QAction *m_mkdirAction = nullptr;
    QAction *m_uploadAction = nullptr;
    QAction *m_downloadAction = nullptr;
    QAction *m_deleteAction = nullptr;
    QAction *m_openAction = nullptr;
    QAction *m_readonlyAction = nullptr;
    QAction *m_themeAction = nullptr;

    QString m_path = QStringLiteral("D:\\");
    QString m_lastLocalDir;
    bool m_connected = false;
    bool m_busy = false;
    quint64 m_progressTotal = 0;
};
