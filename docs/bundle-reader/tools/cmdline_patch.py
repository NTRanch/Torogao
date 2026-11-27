import pathlib
src = pathlib.Path("/tmp/commandline.cs")
s = src.read_text()
s = s.replace('''            "dump" => RunModelCommand(args, dumpMode: true),''',
'''            "dump" => RunModelCommand(args, dumpMode: true),
            "layout" => RunLayoutCommand(args),''')
s = s.replace('''    private static int RunTypeQuery(string[] args)''',
'''    private static int RunLayoutCommand(string[] args)
    {
        var rest = new List<string>(args.Length - 1);
        string? unityVersionText = null;
        string? ggmPath = null;
        for (var i = 1; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--unity-version" when i + 1 < args.Length:
                    unityVersionText = args[++i];
                    break;
                case "--ggm" when i + 1 < args.Length:
                    ggmPath = args[++i];
                    break;
                case var flag when flag.StartsWith("--", StringComparison.Ordinal):
                    throw new CliError(ErrorClasses.UsageError, $"unknown option: {flag}");
                default:
                    rest.Add(args[i]);
                    break;
            }
        }

        if (rest.Count != 3)
        {
            throw new CliError(ErrorClasses.UsageError, UsageText());
        }

        var inputs = InputProbe.Resolve(rest[0], rest[1], unityVersionText, ggmPath);
        using var model = GameModel.Load(inputs);
        var text = LayoutBuilder.Build(model, inputs);
        WriteOutputFile(rest[2], text);
        Console.WriteLine($"wrote {rest[2]}: il2cpp-layout 1");
        return 0;
    }

    private static int RunTypeQuery(string[] args)''')
s = s.replace('''        return "usage: il2cpp-index dump|index [--unity-version V] [--ggm PATH] "''',
'''        return "usage: il2cpp-index dump|index|layout [--unity-version V] [--ggm PATH] "''')
src.write_text(s)
print("patched")
