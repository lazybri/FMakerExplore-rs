#include <QApplication>
#include <QCoreApplication>
#include <QGuiApplication>
#include <QString>

#include <cstddef>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#include "mainwindow.h"
#include "rustclient.h"
#include "theme.h"

#ifdef Q_OS_LINUX
#include "linuxlauncher.h"
#endif

#ifndef FM_VERSION_STRING
#define FM_VERSION_STRING "0.0.0"
#endif

namespace {

bool hasArgument(const std::vector<std::string> &arguments, const char *name)
{
    for (const std::string &argument : arguments) {
        if (argument == name) {
            return true;
        }
    }
    return false;
}

std::vector<std::string> applicationArguments(const std::vector<std::string> &arguments)
{
    std::vector<std::string> filtered;
    filtered.reserve(arguments.size());
    for (const std::string &argument : arguments) {
        if (argument == "--no-elevate" || argument == "--print-elevation"
            || argument == "--smoke-test" || argument == "--fm-elevated"
            || argument.rfind("--fm-env=", 0) == 0) {
            continue;
        }
        filtered.push_back(argument);
    }
    return filtered;
}

void printHelp()
{
    std::printf(
        "Usage: fmaker-explore-gui [--version] [--help] [--smoke-test]\n"
        "       fmaker-explore-gui [--no-elevate] [--print-elevation]\n\n"
        "Qt 6 file manager for the FMakerExplore Rust core.\n\n"
        "  --version           Print the application and Rust core version\n"
        "  --help              Show this help\n"
        "  --smoke-test        Build the UI, switch themes, and exit\n"
        "\n"
        "Linux only:\n"
        "  --no-elevate        Do not request root privileges\n"
        "                      (also FM_EXPLORE_NO_ELEVATE=1)\n"
        "  --print-elevation   Print the pkexec command and exit\n");
}

} // namespace

int main(int argc, char *argv[])
{
    std::vector<std::string> arguments;
    arguments.reserve(argc > 1 ? static_cast<std::size_t>(argc - 1) : 0);
    for (int index = 1; index < argc; ++index) {
        arguments.emplace_back(argv[index]);
    }

#ifdef Q_OS_LINUX
    bool elevatedChild = false;
    arguments = LinuxLauncher::applyForwardedEnvironment(arguments, elevatedChild);
#endif

    const bool smokeTest = hasArgument(arguments, "--smoke-test");
#ifdef Q_OS_LINUX
    const bool noElevate = hasArgument(arguments, "--no-elevate");
    const bool printElevation = hasArgument(arguments, "--print-elevation");
#endif

    for (const std::string &argument : arguments) {
        if (argument == "--version" || argument == "-V") {
            const QByteArray rustVersion = RustClient::version().toUtf8();
            std::printf("fmaker-explore-gui %s (rust core %s)\n", FM_VERSION_STRING,
                        rustVersion.constData());
            return 0;
        }
        if (argument == "--help" || argument == "-h") {
            printHelp();
            return 0;
        }
    }

#ifdef Q_OS_LINUX
    const bool elevationDisabled = noElevate || LinuxLauncher::elevationDisabledByEnvironment();

    if (printElevation) {
        if (LinuxLauncher::isElevated()) {
            std::printf("Already running as root; no elevation needed.\n");
            return 0;
        }
        if (elevationDisabled) {
            std::printf("Elevation is disabled.\n");
            return 0;
        }
        const std::string self = LinuxLauncher::selfExecutable();
        if (self.empty()) {
            std::fprintf(stderr, "error: cannot determine the executable path.\n");
            return 1;
        }
        if (!LinuxLauncher::pkexecAvailable()) {
            std::fprintf(stderr, "note: no trusted pkexec was found; install polkit.\n");
        }
        const std::vector<std::string> command =
            LinuxLauncher::buildElevationCommand(self, arguments);
        std::printf("%s\n", LinuxLauncher::describeCommand(command).c_str());
        return 0;
    }

    if (!smokeTest && !elevationDisabled && !elevatedChild
        && !LinuxLauncher::isElevated() && LinuxLauncher::hasGraphicalSession()) {
        const std::string self = LinuxLauncher::selfExecutable();
        if (self.empty()) {
            std::fprintf(stderr, "warning: cannot determine the executable path; "
                                 "continuing without elevation.\n");
        } else if (LinuxLauncher::pkexecAvailable()) {
            const std::vector<std::string> command =
                LinuxLauncher::buildElevationCommand(self, arguments);
            std::fprintf(stderr, "Requesting root privileges through polkit (pkexec)...\n");
            const int error = LinuxLauncher::execCommand(command);
            std::fprintf(stderr, "warning: could not start pkexec: %s; "
                                 "continuing without elevation.\n",
                         std::strerror(error));
        } else {
            std::fprintf(stderr, "warning: pkexec was not found; continuing without elevation.\n");
        }
    }
#endif

    std::vector<std::string> appArguments = applicationArguments(arguments);
    std::vector<std::string> storage;
    storage.reserve(appArguments.size() + 1);
    storage.emplace_back(argc > 0 && argv[0] != nullptr ? argv[0] : "fmaker-explore-gui");
    storage.insert(storage.end(), appArguments.begin(), appArguments.end());

    std::vector<char *> appArgv;
    appArgv.reserve(storage.size() + 1);
    for (std::string &argument : storage) {
        appArgv.push_back(argument.data());
    }
    appArgv.push_back(nullptr);
    int appArgc = static_cast<int>(storage.size());

    QApplication app(appArgc, appArgv.data());
    QCoreApplication::setOrganizationName(QStringLiteral("FMakerExplore"));
    QCoreApplication::setApplicationName(QStringLiteral("FMakerExplore"));
    QCoreApplication::setApplicationVersion(QStringLiteral(FM_VERSION_STRING));
    QGuiApplication::setDesktopFileName(QStringLiteral("fmaker-explore-gui"));

    Theme::initialize();

    MainWindow window(!smokeTest);
    if (smokeTest) {
        RustClient core;
        const FmReply check = core.call(QStringLiteral("version"));
        if (!check.ok) {
            std::fprintf(stderr, "Rust core self-check failed: %s\n",
                         qPrintable(check.error));
            return 1;
        }
        Theme::apply(true);
        Theme::apply(false);
        return 0;
    }

    window.show();
    return app.exec();
}
