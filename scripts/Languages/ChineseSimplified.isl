; *** Inno Setup version 6.5.0+ Chinese Simplified messages ***
;
; To download user-contributed translations of this file, go to:
;   https://jrsoftware.org/files/istrans/
;
; Maintainer: Zhenghan Yang (Kira)
; Email: 847320916@QQ.com
; Github: https://github.com/kira-96/Inno-Setup-Chinese-Simplified-Translation
; Encoding: UTF-8
;

[LangOptions]
LanguageName=简体中文
LanguageID=$0804
LanguageCodePage=936

[Messages]

; *** Application titles
SetupAppTitle=安装
SetupWindowTitle=安装 - %1
UninstallAppTitle=卸载
UninstallAppFullTitle=%1 卸载

; *** Misc. common
InformationTitle=信息
ConfirmTitle=确认
ErrorTitle=错误

; *** SetupLdr messages
SetupLdrStartupMessage=现在将安装 %1。您想要继续吗？

; *** Startup error messages
SetupFileMissing=安装目录中缺少文件 %1。请修正这个问题或者获取程序的新副本。
SetupFileCorrupt=安装文件已损坏。请获取程序的新副本。
InvalidParameter=无效的命令行参数：%n%n%1
SetupAlreadyRunning=安装程序正在运行。
WindowsVersionNotSupported=此程序不支持当前计算机运行的 Windows 版本。
AdminPrivilegesRequired=在安装此程序时您必须以管理员身份登录。

; *** Setup common messages
ExitSetupTitle=退出安装程序
ExitSetupMessage=安装程序尚未完成。如果现在退出，将不会安装该程序。%n%n您之后可以再次运行安装程序完成安装。%n%n现在退出安装程序吗？
AboutSetupMenuItem=关于安装程序(&A)...

; *** Buttons
ButtonBack=< 上一步(&B)
ButtonNext=下一步(&N)
ButtonInstall=安装(&I)
ButtonOK=确定
ButtonCancel=取消
ButtonYes=是(&Y)
ButtonNo=否(&N)
ButtonFinish=完成(&F)
ButtonBrowse=浏览(&B)...

; *** "Select Language" dialog messages
SelectLanguageTitle=选择安装语言
SelectLanguageLabel=选择安装时使用的语言。

; *** Common wizard text
ClickNext=点击"下一步"继续，或点击"取消"退出安装程序。
BrowseDialogTitle=浏览文件夹
BrowseDialogLabel=在下面的列表中选择一个文件夹，然后点击"确定"。

; *** "Welcome" wizard page
WelcomeLabel1=欢迎使用 [name] 安装向导
WelcomeLabel2=即将在您的计算机上安装 [name/ver]。%n%n建议您在继续安装前关闭所有其他应用程序。

; *** "Select Destination Location" wizard page
WizardSelectDir=选择目标位置
SelectDirDesc=您想将 [name] 安装在哪里？
SelectDirLabel3=安装程序将安装 [name] 到下面的文件夹中。
SelectDirBrowseLabel=点击"下一步"继续。如果您想选择其他文件夹，点击"浏览"。
DiskSpaceMBLabel=至少需要有 [mb] MB 的可用磁盘空间。

; *** "Select Additional Tasks" wizard page
WizardSelectTasks=选择附加任务
SelectTasksDesc=您想要安装程序执行哪些附加任务？

; *** "Select Start Menu Folder" wizard page
WizardSelectProgramGroup=选择开始菜单文件夹
SelectStartMenuFolderDesc=安装程序应该在哪里放置程序的快捷方式？
SelectStartMenuFolderLabel3=安装程序将在下列"开始"菜单文件夹中创建程序的快捷方式。

; *** "Ready to Install" wizard page
WizardReady=准备安装
ReadyLabel1=安装程序准备就绪，现在可以开始安装 [name] 到您的计算机。
ReadyMemoDir=目标位置：
ReadyMemoGroup=开始菜单文件夹：
ReadyMemoTasks=附加任务：

; *** "Installing" wizard page
WizardInstalling=正在安装
InstallingLabel=安装程序正在安装 [name] 到您的计算机，请稍候。

; *** "Setup Completed" wizard page
FinishedHeadingLabel=[name] 安装完成
FinishedLabel=安装程序已在您的计算机中安装了 [name]。您可以通过已安装的快捷方式运行此应用程序。
ClickFinish=点击"完成"退出安装程序。
RunEntryExec=运行 %1

; *** Installation phase messages
SetupAborted=安装程序未完成安装。%n%n请修正这个问题并重新运行安装程序。

; *** Installation status messages
StatusCreateDirs=正在创建目录...
StatusExtractFiles=正在提取文件...
StatusCreateIcons=正在创建快捷方式...
StatusCreateRegistryEntries=正在创建注册表条目...
StatusSavingUninstall=正在保存卸载信息...

; *** File copying errors
ErrorCopying=尝试复制下列文件时出错：

; *** Uninstaller messages
ConfirmUninstall=您确认要完全移除 %1 及其所有组件吗？
UninstallStatusLabel=正在从您的计算机中移除 %1，请稍候。
UninstalledAll=已顺利从您的计算机中移除 %1。

; *** Uninstallation phase messages
WizardUninstalling=卸载状态
StatusUninstalling=正在卸载 %1...

[CustomMessages]
NameAndVersion=%1 版本 %2
CreateDesktopIcon=创建桌面快捷方式(&D)
UninstallProgram=卸载 %1
LaunchProgram=运行 %1
AutoStartProgram=自动启动 %1
