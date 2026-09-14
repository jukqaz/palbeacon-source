namespace PalDataPack.Tests;

public sealed class PackageSampleProbeTests
{
    [Fact]
    public void Item_rarity_probe_accepts_only_the_reviewed_widget_package()
    {
        PackageSampleProbe.ValidatePackagePath(
            PackageSampleProbe.ItemRarityWidgetPackagePath);

        var nearMatch = PackageSampleProbe.ItemRarityWidgetPackagePath
            + "_Copy";
        var failure = Assert.Throws<DataPackFailure>(() =>
            PackageSampleProbe.ValidatePackagePath(nearMatch));

        Assert.Equal(DataPackExitCode.Usage, failure.ExitCode);
    }

    [Theory]
    [InlineData("")]
    [InlineData("Pal/Content/Pal/DataTable/Item/DT_ItemDataTable")]
    [InlineData(
        "pal/Content/Pal/Blueprint/UI/UserInterface/MainMenu/"
        + "InventoryEquipment/WBP_InventoryEquipment_ItemInfo")]
    public void Item_rarity_probe_rejects_other_package_paths(string path)
    {
        var failure = Assert.Throws<DataPackFailure>(() =>
            PackageSampleProbe.ValidatePackagePath(path));

        Assert.Equal(DataPackExitCode.Usage, failure.ExitCode);
    }
}
