using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text;
using Cpp2IL.Core.Model.Contexts;

namespace Il2CppIndex;

/// <summary>
/// Emits the "il2cpp-layout 2" document: every type in the model in
/// declaration order with its serialized-field layout facts, for the
/// estate bundle reader's stripped-MonoBehaviour recovery (rung 2).
///
/// The document is deliberately index-based: types are numbered in
/// emission order and field types reference those numbers, so no
/// name ever needs to survive tokenization. Names ride at line ends.
///
/// Field classification follows Unity's documented serialization
/// rules, applied to metadata facts only: static, literal and
/// [NonSerialized]-flagged fields are excluded; delegates,
/// interfaces, System.Object, native pointers, multi-dimensional
/// arrays, Nullable&lt;T&gt; and non-[Serializable] classes are
/// unserializable (the field is skipped by the serializer, recorded
/// here as an S line with the reason). Anything the classifier
/// cannot decide structurally becomes an R line, which blocks the
/// containing type: a consumer must route it to review, never guess.
///
/// [SerializeField] and [SerializeReference] ARE read, through
/// the library's custom-attribute analysis (the analysis must be
/// invoked per field; the raw attribute cache is empty without
/// it — measured). A field whose attribute data cannot be analyzed
/// becomes an R line: the consumer routes it to review.
/// </summary>
internal static class LayoutBuilder
{
    private const int MaxTypes = 1_000_000;
    private const int MaxFieldsPerType = 100_000;
    private const int MaxBaseChainLength = 1024;
    private const int MaxClassifyDepth = 8;

    // ECMA-335 flag values, used raw because the named enum members
    // are marked obsolete for formatter-based serialization
    // (SYSLIB0050) and this repo builds with warnings as errors. The
    // flags themselves are metadata facts and remain stable.
    private const TypeAttributes SerializableFlag = (TypeAttributes)0x2000;
    private const FieldAttributes NotSerializedFlag = (FieldAttributes)0x0080;

    private static readonly HashSet<string> PrimitiveNames = new(StringComparer.Ordinal)
    {
        "System.Boolean",
        "System.Byte",
        "System.SByte",
        "System.Int16",
        "System.UInt16",
        "System.Int32",
        "System.UInt32",
        "System.Int64",
        "System.UInt64",
        "System.Single",
        "System.Double",
        "System.Char",
        "System.String",
    };

    public static string Build(GameModel model, ResolvedInputs inputs)
    {
        ArgumentNullException.ThrowIfNull(model);
        ArgumentNullException.ThrowIfNull(inputs);

        var types = new List<TypeAnalysisContext>();
        var indexByContext = new Dictionary<TypeAnalysisContext, int>(
            ReferenceEqualityComparer.Instance
        );
        var indexByFullName = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var assembly in model.App.Assemblies)
        {
            foreach (var type in assembly.Types)
            {
                if (types.Count >= MaxTypes)
                {
                    throw new CliError(
                        ErrorClasses.ParseFailure,
                        "layout: type count exceeds the sanity bound"
                    );
                }

                indexByContext[type] = types.Count;
                types.Add(type);
                if (!indexByFullName.ContainsKey(type.FullName))
                {
                    indexByFullName[type.FullName] = types.Count - 1;
                }
            }
        }

        var builder = new StringBuilder();
        builder.Append("il2cpp-layout 2\n");
        var metadataSize = new FileInfo(inputs.MetadataPath).Length;
        builder.Append($"meta {inputs.MetadataHeaderVersion} {metadataSize}\n");
        builder.Append($"types {types.Count}\n");

        var fieldSections = new StringBuilder();
        var errorCount = 0;
        for (var i = 0; i < types.Count; i++)
        {
            var type = types[i];
            string kind;
            var serializable = 0;
            var baseExpr = "-";
            var enumUnderIndex = -1;
            try
            {
                kind = SignatureRenderer.KindOf(type);
                serializable =
                    (type.Attributes & SerializableFlag) != 0 ? 1 : 0;
                if (type.BaseType is not null)
                {
                    baseExpr = BaseExpr(type.BaseType, indexByContext, indexByFullName);
                }

                if (type.IsEnumType && type.EnumUnderlyingType is not null)
                {
                    enumUnderIndex = IndexOf(
                        type.EnumUnderlyingType,
                        indexByContext,
                        indexByFullName
                    );
                }
            }
            catch (Exception)
            {
                kind = "error";
                errorCount++;
            }

            builder.Append(
                $"T {i} {kind} {serializable} {enumUnderIndex} {baseExpr}\n"
            );
            builder.Append($"A {i} {TextEscaping.Escape(type.DeclaringAssembly.Name)}\n");
            var ns = type.Namespace.Length == 0 ? "-" : TextEscaping.Escape(type.Namespace);
            builder.Append($"N {i} {ns} {TextEscaping.Escape(type.Name)}\n");

            if (kind == "error" || type.IsEnumType)
            {
                fieldSections.Append($"D {i} 0\n");
                continue;
            }

            List<FieldAnalysisContext> fields;
            try
            {
                fields = type.Fields;
            }
            catch (Exception)
            {
                fieldSections.Append($"D {i} 0\n");
                errorCount++;
                continue;
            }

            if (fields.Count > MaxFieldsPerType)
            {
                throw new CliError(
                    ErrorClasses.ParseFailure,
                    $"layout: type {type.FullName} exceeds the field sanity bound"
                );
            }

            var lines = new List<string>(fields.Count);
            foreach (var field in fields)
            {
                lines.Add(
                    FieldLine(field, indexByContext, indexByFullName)
                );
            }

            fieldSections.Append($"D {i} {lines.Count}\n");
            foreach (var line in lines)
            {
                fieldSections.Append(line).Append('\n');
            }
        }

        builder.Append(fieldSections);
        if (errorCount > 0)
        {
            Console.Error.WriteLine($"layout: {errorCount} types failed to describe");
        }

        return builder.ToString();
    }

    private static int IndexOf(
        TypeAnalysisContext type,
        Dictionary<TypeAnalysisContext, int> indexByContext,
        Dictionary<string, int> indexByFullName
    )
    {
        if (indexByContext.TryGetValue(type, out var index))
        {
            return index;
        }

        return indexByFullName.TryGetValue(type.FullName, out var byName) ? byName : -1;
    }

    /// <summary>
    /// The base-type expression for a T line: `-` (none),
    /// `t &lt;idx&gt;` (a plain type), `g &lt;def-idx&gt; &lt;nargs&gt;
    /// &lt;arg-ftype&gt;...` (a constructed generic base — the
    /// substitution source for the definition's parameter-typed
    /// fields), or `x` (a base the grammar cannot describe; the
    /// consumer must route derivations through it to review).
    /// </summary>
    private static string BaseExpr(
        TypeAnalysisContext baseType,
        Dictionary<TypeAnalysisContext, int> indexByContext,
        Dictionary<string, int> indexByFullName
    )
    {
        if (baseType is GenericInstanceTypeAnalysisContext generic)
        {
            if (generic.GenericType is null || generic.GenericArguments is null)
            {
                return "x";
            }

            var defIndex = IndexOf(generic.GenericType, indexByContext, indexByFullName);
            if (defIndex < 0)
            {
                return "x";
            }

            var parts = new List<string> { $"g {defIndex} {generic.GenericArguments.Count}" };
            foreach (var argument in generic.GenericArguments)
            {
                var argType = Classify(argument, 0, indexByContext, indexByFullName);
                if (
                    argType.StartsWith("skip ", StringComparison.Ordinal)
                    || argType.StartsWith("rev ", StringComparison.Ordinal)
                )
                {
                    // A named reference type bound as a generic
                    // argument (an event's args class, a datum
                    // class): emit it as a reference whose field
                    // treatment the consumer decides from its base
                    // chain. Anything else undescribable becomes a
                    // blocked marker: the base chain stays walkable
                    // and only a field substituting that parameter
                    // is blocked downstream.
                    if (argument.GetType() == typeof(TypeAnalysisContext))
                    {
                        var refIndex = IndexOf(argument, indexByContext, indexByFullName);
                        if (refIndex >= 0)
                        {
                            parts.Add($"ref {refIndex}");
                            continue;
                        }
                    }

                    parts.Add("blk");
                    continue;
                }

                parts.Add(argType);
            }

            return string.Join(" ", parts);
        }

        if (baseType.GetType() != typeof(TypeAnalysisContext))
        {
            return "x";
        }

        var index = IndexOf(baseType, indexByContext, indexByFullName);
        return index < 0 ? "x" : $"t {index}";
    }

    private static string FieldLine(
        FieldAnalysisContext field,
        Dictionary<TypeAnalysisContext, int> indexByContext,
        Dictionary<string, int> indexByFullName
    )
    {
        var name = TextEscaping.Escape(field.Name);
        var attributes = field.Attributes;
        if (attributes.HasFlag(FieldAttributes.Static))
        {
            return $"S static {name}";
        }

        if (attributes.HasFlag(FieldAttributes.Literal))
        {
            return $"S literal {name}";
        }

        if (attributes.HasFlag(NotSerializedFlag))
        {
            return $"S notser {name}";
        }

        bool serializeField;
        bool serializeReference;
        try
        {
            field.AnalyzeCustomAttributeData();
            serializeField = field.HasCustomAttributeWithFullName(
                "UnityEngine.SerializeField"
            );
            serializeReference = field.HasCustomAttributeWithFullName(
                "UnityEngine.SerializeReference"
            );
        }
        catch (Exception)
        {
            // Attribute data that cannot be analyzed means the
            // field's serialization is undecidable from metadata:
            // block the type rather than guess.
            return $"R attrfail {name}";
        }

        var visibility = attributes.HasFlag(FieldAttributes.Public) ? "pub" : "nonpub";
        var ftype = Classify(field.FieldType, 0, indexByContext, indexByFullName);
        if (ftype.StartsWith("skip ", StringComparison.Ordinal))
        {
            return $"S {ftype.Substring(5)} {name}";
        }

        if (ftype.StartsWith("rev ", StringComparison.Ordinal))
        {
            return $"R {ftype.Substring(4)} {name}";
        }

        return $"F {visibility} {(serializeField ? 1 : 0)} {(serializeReference ? 1 : 0)} {ftype} {name}";
    }

    /// <summary>
    /// Classifies a field type into the layout grammar:
    /// prim/enum/pptr/inl &lt;type-index&gt;, arr/lst &lt;inner&gt;,
    /// dct &lt;key&gt; &lt;value&gt;, or a skip/rev verdict with a
    /// fixed-vocabulary reason token.
    /// </summary>
    private static string Classify(
        TypeAnalysisContext type,
        int depth,
        Dictionary<TypeAnalysisContext, int> indexByContext,
        Dictionary<string, int> indexByFullName
    )
    {
        if (depth > MaxClassifyDepth)
        {
            return "rev depth";
        }

        if (type is SzArrayTypeAnalysisContext szArray)
        {
            var inner = Classify(szArray.ElementType, depth + 1, indexByContext, indexByFullName);
            return inner.StartsWith("skip ", StringComparison.Ordinal)
                || inner.StartsWith("rev ", StringComparison.Ordinal)
                ? inner
                : $"arr {inner}";
        }

        if (type is GenericParameterTypeAnalysisContext genericParameter)
        {
            return $"par {genericParameter.Index}";
        }

        if (type is GenericInstanceTypeAnalysisContext generic)
        {
            var definition = generic.GenericType?.FullName;
            var arguments = generic.GenericArguments;
            if (generic.GenericType is not null && IsDelegate(generic.GenericType))
            {
                return "skip delegate";
            }

            if (generic.GenericType is not null && generic.GenericType.IsInterface)
            {
                return "skip interface";
            }

            if (
                definition == "System.Collections.Generic.List`1"
                && arguments is { Count: 1 }
            )
            {
                var inner = Classify(arguments[0], depth + 1, indexByContext, indexByFullName);
                return inner.StartsWith("skip ", StringComparison.Ordinal)
                    || inner.StartsWith("rev ", StringComparison.Ordinal)
                    ? inner
                    : $"lst {inner}";
            }

            if (definition == "System.Collections.Generic.HashSet`1")
            {
                // Same doctrine as Dictionary: Unity's serializer
                // does not serialize HashSet fields.
                return "skip hashset";
            }

            if (definition == "System.Collections.Generic.Dictionary`2")
            {
                // Unity's serializer does not serialize Dictionary
                // fields at all (documented; it is precisely why
                // SerializedDictionary-style wrapper classes with
                // list fields exist — those classify through their
                // own list fields, not this branch).
                return "skip dictionary";
            }

            if (definition == "System.Nullable`1")
            {
                return "skip nullable";
            }

            if (
                generic.GenericType is not null
                && arguments is not null
                && IsInlineEligible(generic.GenericType)
            )
            {
                var defIndex = IndexOf(
                    generic.GenericType,
                    indexByContext,
                    indexByFullName
                );
                if (defIndex < 0)
                {
                    return "rev unknown-type";
                }

                var parts = new List<string> { $"gin {defIndex} {arguments.Count}" };
                foreach (var argument in arguments)
                {
                    var argType = Classify(
                        argument,
                        depth + 1,
                        indexByContext,
                        indexByFullName
                    );
                    if (
                        argType.StartsWith("skip ", StringComparison.Ordinal)
                        || argType.StartsWith("rev ", StringComparison.Ordinal)
                    )
                    {
                        // An undescribable argument does not veto
                        // the instance: the definition's serialized
                        // shape may never use that parameter (the
                        // UnityEvent`2<String, Collision> case —
                        // Collision is native-backed, yet the
                        // event's persistent calls serialize
                        // without it). Emit a blocked marker; the
                        // consumer blocks only a field that
                        // actually substitutes the parameter.
                        parts.Add("blk");
                        continue;
                    }

                    parts.Add(argType);
                }

                return string.Join(" ", parts);
            }

            return "skip nonserclass";
        }

        if (type.GetType() != typeof(TypeAnalysisContext))
        {
            // Array (multi-dim), pointer, by-ref, generic-parameter
            // and any other context subclass: not serializable in a
            // form this grammar can state.
            return type is ArrayTypeAnalysisContext ? "skip multidim" : "rev context";
        }

        var index = IndexOf(type, indexByContext, indexByFullName);
        if (index < 0)
        {
            return "rev unknown-type";
        }

        var fullName = type.FullName;
        if (PrimitiveNames.Contains(fullName))
        {
            return $"prim {index}";
        }

        if (type.IsEnumType)
        {
            return $"enum {index}";
        }

        if (IsDelegate(type))
        {
            return "skip delegate";
        }

        if (type.IsInterface)
        {
            return "skip interface";
        }

        if (fullName == "System.Object")
        {
            return "skip object";
        }

        if (fullName == "UnityEngine.Object")
        {
            // A field typed exactly UnityEngine.Object (e.g.
            // UnityEvent PersistentCall.m_Target) serializes as a
            // PPtr like any engine-object reference; the derived-
            // type walk below starts at BaseType and would miss it.
            return $"pptr {index}";
        }

        if (fullName is "System.IntPtr" or "System.UIntPtr")
        {
            return "skip intptr";
        }

        if (fullName == "System.Decimal")
        {
            return "skip decimal";
        }

        if (DerivesFromUnityObject(type))
        {
            return $"pptr {index}";
        }

        if (IsInlineEligible(type))
        {
            return $"inl {index}";
        }

        return "skip nonserclass";
    }

    /// <summary>
    /// Whether a named type serializes inline as a field: value
    /// types when flagged [Serializable] or Unity-namespaced (the
    /// flag is not carried for engine structs in IL2CPP metadata —
    /// UnityEngine.Color reads back flag-less yet serializes,
    /// corpus-proven; a flag-less game-code struct does not
    /// serialize, fixture-proven by RootMotion's Muscle.State);
    /// classes when flagged [Serializable] (the flag is inherited)
    /// or when they are native-serialized engine classes
    /// (fixture-proven for AnimationCurve in PES).
    /// </summary>
    private static bool IsInlineEligible(TypeAnalysisContext type)
    {
        if (type.IsValueType)
        {
            // A managed struct serializes only when flagged
            // [Serializable] — a flag-less game-code struct (the
            // RootMotion Muscle.State case: public field, struct
            // type, no flag; the fixture export carries no such
            // field) is skipped by the serializer. Engine-native
            // structs are the exception: they carry no flag in
            // metadata yet serialize (UnityEngine.Color is the
            // proof), through native shapes or their declared
            // fields, so Unity-namespaced structs stay eligible.
            if ((type.Attributes & SerializableFlag) != 0)
            {
                return true;
            }

            return type.FullName.StartsWith("UnityEngine.", StringComparison.Ordinal)
                || type.FullName.StartsWith("Unity.", StringComparison.Ordinal);
        }

        // [Serializable] is an inherited attribute: a class whose
        // base carries it (UnityEvent subclasses are the corpus
        // case — UnityEvent itself is [Serializable], its
        // subclasses usually do not re-declare the flag in
        // metadata) serializes inline. Walk the chain, unwrapping
        // generic instances like the engine-object walk does —
        // but stop below System.Object / System.ValueType: both
        // carry the flag in BCL metadata, and counting them would
        // make every type in the model "serializable".
        TypeAnalysisContext? current = type;
        for (var i = 0; i < MaxBaseChainLength && current is not null; i++)
        {
            if (current is GenericInstanceTypeAnalysisContext instance)
            {
                current = instance.GenericType;
                if (current is null)
                {
                    break;
                }
            }

            if (current.FullName is "System.Object" or "System.ValueType")
            {
                break;
            }

            if (current.FullName == "UnityEngine.Events.UnityEventBase")
            {
                // UnityEventBase-derived types are a documented
                // serializable category (event fields serialize
                // as their persistent-call groups); the metadata
                // flag bit is not reliably set on the engine's
                // event types, so the category is named outright
                // like AnimationCurve/Gradient below.
                return true;
            }

            if ((current.Attributes & SerializableFlag) != 0)
            {
                return true;
            }

            current = current.BaseType;
        }

        return type.FullName is "UnityEngine.AnimationCurve" or "UnityEngine.Gradient";
    }

    private static bool IsDelegate(TypeAnalysisContext type)
    {
        var current = type;
        for (var i = 0; i < MaxBaseChainLength && current is not null; i++)
        {
            if (current.FullName == "System.MulticastDelegate")
            {
                return true;
            }

            current = current.BaseType;
        }

        return false;
    }

    private static bool DerivesFromUnityObject(TypeAnalysisContext type)
    {
        // Walk the base chain, unwrapping generic instances to
        // their definitions at each step: a constructed base like
        // RenderPipelineAsset<UniversalRenderPipeline> surfaces
        // as a generic-instance context whose own BaseType does
        // not reliably continue the chain, while its definition's
        // does.
        TypeAnalysisContext? current = type;
        for (var i = 0; i < MaxBaseChainLength && current is not null; i++)
        {
            if (current is GenericInstanceTypeAnalysisContext instance)
            {
                current = instance.GenericType;
                if (current is null)
                {
                    return false;
                }
            }

            if (current.FullName == "UnityEngine.Object")
            {
                return true;
            }

            current = current.BaseType;
        }

        return false;
    }
}
