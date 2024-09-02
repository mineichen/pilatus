# Images
## DynamicImages vs TypedImages

- ImageComponent identifies different kinds of images per measurement (e.g. NIR or Dept image, 2Dimage)
- Having Project-Specific Images (e.g. LidarImage with NIR, Depth, reflectance...) is not feasible, as every filter would have to know all projects
 - Frontend has to list possible images anyway, to show a list of options
- 
Pro dynamic
 - Frontend can just ask for a Image
 - No Race-Conditions (between asking which Streams exist and asking for images)
 - Less Messages per Actor (just a single StreamDynamic suffices)
 - Easy to implement for Devices, which do not statically know their PixelDept (e.g. Cameras)
 - Asking for multiple ImageComponents at the same time requires Dynamic images eventually... Otherwise there is a explosion of possibilities for cameras (e.g. Tuple(Luma8Image, Luma16Image))
 - Could even handle single or multiple ImageComponent images at the same time
 - Filters can limit, which component they filter if more than 1 is requested

Contra dynamic
 - Actors have to handle the case, if incompatible formats are provided despite asking for something else
 - Pixel-Depth has to be part of the query beside ImageComponent or the callee has to decide


Conclusion: There should be a common, dynamic image stream. For very common formats as Luma8, a dedicated stream could be introduced later.